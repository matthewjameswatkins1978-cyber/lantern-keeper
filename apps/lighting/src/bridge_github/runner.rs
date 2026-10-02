use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tracing::{error, info, warn};

use super::executor::BridgeExecutor;
use super::queue::{BridgeStatusFile, GitQueue, QueueError};
use super::schema::{MirrorRefreshStatus, ReceiptStatus};
use super::state::BridgeStateStore;
use super::tethers::TethersGate;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub service_url: String,
    pub post_repo_path: PathBuf,
    pub git_repo_path: Option<PathBuf>,
    pub state_path: PathBuf,
    pub poll_interval_secs: u64,
    pub push: bool,
    pub audit_token: Option<String>,
    pub tethers_engine_path: Option<PathBuf>,
}

pub struct BridgeRunner {
    config: BridgeConfig,
    queue: GitQueue,
    tethers: TethersGate,
    executor: BridgeExecutor,
}

impl BridgeRunner {
    pub fn new(config: BridgeConfig) -> Self {
        let queue = GitQueue::new(&config.post_repo_path);
        let tethers = TethersGate::new(
            config.service_url.clone(),
            config.audit_token.clone(),
            config.tethers_engine_path.clone(),
        );
        let executor = BridgeExecutor::new(
            config.service_url.clone(),
            tethers.clone(),
            config.git_repo_path.clone(),
        );

        Self {
            config,
            queue,
            tethers,
            executor,
        }
    }

    pub async fn run_once(&self) -> Result<()> {
        info!(
            post_repo = %self.config.post_repo_path.display(),
            service_url = %self.config.service_url,
            "Starting Lantern GitHub Bridge cycle (once mode)"
        );

        // 1. Verify repository remote identity and privacy
        self.queue
            .verify_repository(self.config.push)
            .context("failed post-repo repository verification")?;

        // 2. Fetch remote inbox
        if self.config.push
            && let Err(e) = self.queue.fetch_remote()
        {
            warn!("git fetch origin failed: {e}; proceeding with local state if available");
        }

        // 3. Load durable state
        let mut state = BridgeStateStore::load_or_create(&self.config.state_path)
            .context("failed to load durable bridge state")?;

        // 4. Inspect inbox HEAD
        let inbox_head = match self.queue.get_inbox_head() {
            Ok(h) => h,
            Err(e) => {
                let err_msg = format!("could not resolve inbox HEAD: {e}");
                let _ = state.record_cycle(false, Some(err_msg.clone()));
                bail!(err_msg);
            }
        };

        let checkpoint = state.data.last_processed_inbox_commit.clone();
        info!(
            checkpoint = checkpoint.as_deref().unwrap_or("<none>"),
            inbox_head = %inbox_head,
            "Comparing checkpoint against inbox HEAD"
        );

        // 5. Enumerate new commits oldest-first
        let commits_to_process = match self
            .queue
            .list_commits_between(checkpoint.as_deref(), &inbox_head)
        {
            Ok(commits) => commits,
            Err(QueueError::NonDescendantInboxHead {
                checkpoint,
                inbox_head,
            }) => {
                let err_msg = format!(
                    "SUSPECT: inbox HEAD '{inbox_head}' is not a descendant of checkpoint '{checkpoint}'. STOPPING MUTATIONS!"
                );
                error!("{err_msg}");
                let _ = state.record_cycle(false, Some(err_msg.clone()));
                bail!(err_msg);
            }
            Err(e) => {
                let err_msg = format!("failed to list commits: {e}");
                let _ = state.record_cycle(false, Some(err_msg.clone()));
                bail!(err_msg);
            }
        };

        if commits_to_process.is_empty() {
            let (lantern_reachable, lantern_version) = self.probe_lantern().await;
            let authority_path_reachable = self.tethers.probe_authority_path().await;
            let tethers_engine_present = self.tethers.is_engine_available();
            let mirror_gen_at = self.get_mirror_generated_at();

            let mutation_ready = lantern_reachable
                && authority_path_reachable
                && state.data.last_terminal_error.is_none();

            let status_file = BridgeStatusFile {
                bridge_version: super::executor::BRIDGE_VERSION.to_string(),
                last_cycle_at: Some(Utc::now().to_rfc3339()),
                last_success_at: Some(Utc::now().to_rfc3339()),
                last_processed_inbox_commit: state.data.last_processed_inbox_commit.clone(),
                inbox_head: Some(inbox_head.clone()),
                pending_count: 0,
                last_terminal_error: None,
                lantern_reachable,
                tethers_reachable: authority_path_reachable,
                authority_path_reachable,
                tethers_engine_present,
                mutation_ready,
                last_receipt_outcome: state.get_last_receipt_outcome(),
                lantern_version,
                datastore_mode: Some("embedded-surrealkv".to_string()),
                surrealdb_expected_version: Some(crate::EXPECTED_SURREALDB_VERSION.to_string()),
                mirror_last_generated_at: mirror_gen_at,
            };

            let _ = self
                .queue
                .write_receipts_and_status(&[], &status_file, self.config.push);

            info!("No new inbox commits to process. Bridge is up to date (status refreshed).");
            let _ = state.record_cycle(true, None);
            return Ok(());
        }

        info!(
            count = commits_to_process.len(),
            "Processing inbox commits oldest first"
        );

        let mut batch_receipts = Vec::new();
        let mut need_mirror_refresh = false;

        for commit_sha in &commits_to_process {
            info!(commit = %commit_sha, "Inspecting commit");
            let discovered = match self.queue.extract_intent_from_commit(commit_sha) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    info!(commit = %commit_sha, "Commit contains no intent files under intents/; skipping");
                    continue;
                }
                Err(QueueError::ForbiddenModification { commit, path }) => {
                    let err_msg = format!(
                        "SUSPECT: Commit {commit} modified or deleted existing intent file: {path}"
                    );
                    error!("{err_msg}");
                    let _ = state.record_cycle(false, Some(err_msg.clone()));
                    bail!(err_msg);
                }
                Err(e) => {
                    let err_msg = format!("failed to extract intent from commit {commit_sha}: {e}");
                    error!("{err_msg}");
                    let _ = state.record_cycle(false, Some(err_msg.clone()));
                    bail!(err_msg);
                }
            };

            let receipt = self
                .executor
                .process_discovered_intent(&discovered, &mut state)
                .await;

            info!(
                intent_id = %receipt.intent_id,
                status = receipt.status.as_str(),
                authority = receipt.authority.decision,
                "Intent processed"
            );

            if receipt.status == ReceiptStatus::Applied
                && receipt.mirror_refresh == MirrorRefreshStatus::Requested
            {
                need_mirror_refresh = true;
            }

            batch_receipts.push(receipt);
        }

        // 6. Write receipts and bridge status file
        let (lantern_reachable, lantern_version) = self.probe_lantern().await;
        let authority_path_reachable = self.tethers.probe_authority_path().await;
        let tethers_engine_present = self.tethers.is_engine_available();
        let mirror_gen_at = self.get_mirror_generated_at();

        let mutation_ready = lantern_reachable
            && authority_path_reachable
            && state.data.last_terminal_error.is_none();

        let status_file = BridgeStatusFile {
            bridge_version: super::executor::BRIDGE_VERSION.to_string(),
            last_cycle_at: Some(Utc::now().to_rfc3339()),
            last_success_at: Some(Utc::now().to_rfc3339()),
            last_processed_inbox_commit: Some(inbox_head.clone()),
            inbox_head: Some(inbox_head.clone()),
            pending_count: 0,
            last_terminal_error: None,
            lantern_reachable,
            tethers_reachable: authority_path_reachable,
            authority_path_reachable,
            tethers_engine_present,
            mutation_ready,
            last_receipt_outcome: state.get_last_receipt_outcome(),
            lantern_version,
            datastore_mode: Some("embedded-surrealkv".to_string()),
            surrealdb_expected_version: Some(crate::EXPECTED_SURREALDB_VERSION.to_string()),
            mirror_last_generated_at: mirror_gen_at,
        };

        if !batch_receipts.is_empty() {
            let receipt_commit = self
                .queue
                .write_receipts_and_status(&batch_receipts, &status_file, self.config.push)
                .context("failed to write and push receipts")?;

            info!(
                receipt_commit = receipt_commit.as_deref().unwrap_or("<unchanged>"),
                count = batch_receipts.len(),
                "Durable receipts recorded"
            );
        }

        // 7. Safely advance durable checkpoint to inbox_head
        state
            .advance_checkpoint(&inbox_head)
            .context("failed to advance durable checkpoint")?;
        info!(checkpoint = %inbox_head, "Checkpoint advanced safely");

        // 8. Coalesced mirror refresh if any mutations were applied
        if need_mirror_refresh && let Some(ref git_repo) = self.config.git_repo_path {
            info!(
                mirror_repo = %git_repo.display(),
                "Mutations succeeded; performing coalesced mirror refresh"
            );
            Self::refresh_mirror(git_repo, self.config.push).await?;
        }

        // 9. Cycle completed successfully
        state
            .record_cycle(true, None)
            .context("failed to record clean cycle")?;
        info!("Bridge cycle finished cleanly.");

        Ok(())
    }

    pub async fn run_daemon(&self) -> Result<()> {
        info!(
            poll_interval_secs = self.config.poll_interval_secs,
            "Entering Lantern GitHub Bridge daemon loop"
        );

        let mut backoff = Duration::from_secs(self.config.poll_interval_secs);
        let min_interval = Duration::from_secs(self.config.poll_interval_secs);
        let max_interval = Duration::from_secs(120);

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Shutdown signal received. Exiting Lantern GitHub Bridge daemon.");
                    break;
                }
                _ = tokio::time::sleep(backoff) => {
                    match self.run_once().await {
                        Ok(()) => {
                            backoff = min_interval;
                        }
                        Err(e) => {
                            error!("Bridge cycle failed: {e:#}");
                            backoff = std::cmp::min(backoff * 2, max_interval);
                            warn!(retry_in_secs = backoff.as_secs(), "Backing off before next cycle");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn probe_lantern(&self) -> (bool, Option<String>) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        let version_url = format!(
            "{}/api/v1/version",
            self.config.service_url.trim_end_matches('/')
        );
        match client.get(&version_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let v = resp.json::<serde_json::Value>().await.ok().and_then(|j| {
                    j.get("version")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                });
                (true, v)
            }
            _ => (false, None),
        }
    }

    fn get_mirror_generated_at(&self) -> Option<String> {
        self.config.git_repo_path.as_ref().and_then(|p| {
            let sf = p.join("mirror").join("status.json");
            let mf = p.join("mirror").join("manifest.json");
            if sf.exists() {
                std::fs::read_to_string(sf)
                    .ok()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                    .and_then(|j| {
                        j.get("generated_at")
                            .and_then(|s| s.as_str())
                            .map(str::to_string)
                    })
            } else if mf.exists() {
                std::fs::read_to_string(mf)
                    .ok()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                    .and_then(|j| {
                        j.get("generated_at")
                            .and_then(|s| s.as_str())
                            .map(str::to_string)
                    })
            } else {
                None
            }
        })
    }

    pub async fn refresh_mirror(mirror_repo: &Path, push: bool) -> Result<()> {
        let sync_script = Path::new("scripts").join("lantern_git_sync.ps1");
        let export_script = Path::new("scripts").join("lantern_git_export.py");

        if sync_script.exists() {
            let mut cmd = std::process::Command::new("pwsh");
            cmd.arg(&sync_script)
                .arg("-RepoPath")
                .arg(mirror_repo.display().to_string());
            if push {
                cmd.arg("-Push");
            }
            let output = cmd.output()?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                let out = String::from_utf8_lossy(&output.stdout);
                bail!("lantern_git_sync.ps1 failed: {err}\n{out}");
            }
        } else if export_script.exists() {
            let output = std::process::Command::new("python")
                .arg(&export_script)
                .arg("--output")
                .arg(mirror_repo.display().to_string())
                .output()?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                bail!("lantern_git_export.py failed: {err}");
            }
        }

        Ok(())
    }
}
