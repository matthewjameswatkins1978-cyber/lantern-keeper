use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::queue::{EXPECTED_POST_REPO, GitQueue, INBOX_BRANCH, RECEIPTS_BRANCH};
use super::state::BridgeStateStore;
use super::tethers::TethersGate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub lantern_service_url: String,
    pub lantern_reachable: bool,
    pub lantern_version: Option<String>,
    pub canonical_repo_identity: String,
    pub post_repo_path: String,
    pub post_repo_remote_url: Option<String>,
    pub post_repo_expected: String,
    pub post_repo_private: Option<bool>,
    pub post_repo_valid: bool,
    pub inbox_branch_exists: bool,
    pub receipts_branch_exists: bool,
    pub authority_path_reachable: bool,
    pub tethers_engine_present: bool,
    pub tethers_available: bool,
    pub tethers_engine_path: Option<String>,
    pub durable_checkpoint: Option<String>,
    pub inbox_head: Option<String>,
    pub pending_intents_count: usize,
    pub last_successful_cycle: Option<DateTime<Utc>>,
    pub last_terminal_error: Option<String>,
    pub last_receipt_outcome: Option<String>,
    pub datastore_mode_observed: Option<String>,
    pub datastore_mode_configured: String,
    pub surrealdb_expected_version: String,
    pub surrealdb_observed_version: Option<String>,
    pub runtime_binary_version: String,
    pub mirror_repo_path: Option<String>,
    pub mirror_generated_at: Option<String>,
    pub mutation_safe_to_enable: bool,
}

impl DoctorReport {
    pub async fn run(
        service_url: &str,
        post_repo_path: &Path,
        mirror_repo_path: Option<&Path>,
        state_path: &Path,
    ) -> Self {
        let clean_service_url = service_url.trim_end_matches('/');

        // 1. Lantern Service check
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let version_url = format!("{clean_service_url}/api/v1/version");
        let (
            lantern_reachable,
            lantern_version,
            datastore_mode_observed,
            surrealdb_observed_version,
        ) = match client.get(&version_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let j = resp.json::<serde_json::Value>().await.ok();
                let v = j.as_ref().and_then(|j| {
                    j.get("version")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                });
                let dm = j.as_ref().and_then(|j| {
                    j.get("datastore_mode")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                });
                let sov = j.as_ref().and_then(|j| {
                    j.get("surrealdb_observed_version")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                });
                (true, v, dm, sov)
            }
            _ => (false, None, None, None),
        };

        // 2. Canonical repository identity
        let canonical_repo_identity = "matthewjameswatkins1978-cyber/lantern-keeper".to_string();

        // 3. Post repo checks
        let queue = GitQueue::new(post_repo_path);
        let post_repo_remote_url = Command::new("git")
            .current_dir(post_repo_path)
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
                } else {
                    None
                }
            });

        let post_repo_private = Command::new("gh")
            .args([
                "repo",
                "view",
                EXPECTED_POST_REPO,
                "--json",
                "isPrivate",
                "--jq",
                ".isPrivate",
            ])
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Some(s == "true")
                } else {
                    None
                }
            });

        let branch_list = Command::new("git")
            .current_dir(post_repo_path)
            .args(["branch", "-a"])
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
            .unwrap_or_default();

        let inbox_branch_exists = branch_list.contains(INBOX_BRANCH);
        let receipts_branch_exists = branch_list.contains(RECEIPTS_BRANCH);

        let post_repo_valid = post_repo_path.join(".git").exists()
            && post_repo_remote_url
                .as_ref()
                .map(|u| u.contains(EXPECTED_POST_REPO))
                .unwrap_or(false)
            && inbox_branch_exists
            && receipts_branch_exists;

        // 4. Tethers / Authority check
        let tethers = TethersGate::new(clean_service_url.to_string(), None, None);
        let authority_path_reachable = tethers.probe_authority_path().await;
        let engine_path = TethersGate::find_default_engine_path();
        let tethers_engine_present = engine_path.is_some();
        let tethers_available = authority_path_reachable;

        // 5. State / Checkpoint check
        let state = BridgeStateStore::load_or_create(state_path).ok();
        let durable_checkpoint = state
            .as_ref()
            .and_then(|s| s.data.last_processed_inbox_commit.clone());
        let last_successful_cycle = state.as_ref().and_then(|s| s.data.last_success_at);
        let last_terminal_error = state
            .as_ref()
            .and_then(|s| s.data.last_terminal_error.clone());
        let last_receipt_outcome = state.as_ref().and_then(|s| s.get_last_receipt_outcome());

        let inbox_head = queue.get_inbox_head().ok();

        let pending_intents_count = match (&inbox_head, &durable_checkpoint) {
            (Some(head), cp) => queue
                .list_commits_between(cp.as_deref(), head)
                .map(|commits| commits.len())
                .unwrap_or(0),
            _ => 0,
        };

        // 6. Mirror check
        let mirror_generated_at = mirror_repo_path.and_then(|p| {
            let status_file = p.join("mirror").join("status.json");
            let manifest_file = p.join("mirror").join("manifest.json");

            if status_file.exists() {
                std::fs::read_to_string(status_file)
                    .ok()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                    .and_then(|j| {
                        j.get("generated_at")
                            .and_then(|s| s.as_str())
                            .map(str::to_string)
                    })
            } else if manifest_file.exists() {
                std::fs::read_to_string(manifest_file)
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
        });

        // Safe to enable mutation when:
        // - Lantern reachable
        // - Authority path reachable
        // - Post repo is valid (git, branches, remote)
        // - Post repo is verified private
        // - Inbox head is known
        // - No unhandled terminal error
        let mutation_safe_to_enable = lantern_reachable
            && authority_path_reachable
            && post_repo_valid
            && post_repo_private == Some(true)
            && inbox_head.is_some()
            && last_terminal_error.is_none();

        Self {
            lantern_service_url: clean_service_url.to_string(),
            lantern_reachable,
            lantern_version,
            canonical_repo_identity,
            post_repo_path: post_repo_path.display().to_string(),
            post_repo_remote_url,
            post_repo_expected: EXPECTED_POST_REPO.to_string(),
            post_repo_private,
            post_repo_valid,
            inbox_branch_exists,
            receipts_branch_exists,
            authority_path_reachable,
            tethers_engine_present,
            tethers_available,
            tethers_engine_path: engine_path.map(|p| p.display().to_string()),
            durable_checkpoint,
            inbox_head,
            pending_intents_count,
            last_successful_cycle,
            last_terminal_error,
            last_receipt_outcome,
            datastore_mode_observed,
            datastore_mode_configured: "embedded-surrealkv".to_string(),
            surrealdb_expected_version: crate::EXPECTED_SURREALDB_VERSION.to_string(),
            surrealdb_observed_version,
            runtime_binary_version: env!("CARGO_PKG_VERSION").to_string(),
            mirror_repo_path: mirror_repo_path.map(|p| p.display().to_string()),
            mirror_generated_at,
            mutation_safe_to_enable,
        }
    }

    pub fn print_diagnostics(&self, json: bool) {
        if json {
            if let Ok(serialized) = serde_json::to_string_pretty(self) {
                println!("{serialized}");
            }
            return;
        }

        println!("=== Lantern GitHub Bridge Doctor ===");
        println!(
            "Lantern Service:            {} (reachable: {}, version: {})",
            self.lantern_service_url,
            if self.lantern_reachable { "YES" } else { "NO" },
            self.lantern_version.as_deref().unwrap_or("unknown")
        );
        println!(
            "Datastore Mode (observed):  {}",
            self.datastore_mode_observed
                .as_deref()
                .unwrap_or("<unreachable or unknown>")
        );
        println!(
            "Datastore Mode (configured):{}",
            self.datastore_mode_configured
        );
        println!(
            "SurrealDB Expected Version: {}",
            self.surrealdb_expected_version
        );
        println!(
            "SurrealDB Observed Version: {}",
            self.surrealdb_observed_version
                .as_deref()
                .unwrap_or("<unreachable or unknown>")
        );
        println!(
            "Runtime Version:            {}",
            self.runtime_binary_version
        );
        println!(
            "Canonical Repo:             {}",
            self.canonical_repo_identity
        );
        println!(
            "Post Repo Valid:            {}",
            if self.post_repo_valid { "YES" } else { "NO" }
        );
        println!("Post Repo Path:             {}", self.post_repo_path);
        println!(
            "Post Remote URL:            {} (expected: {})",
            self.post_repo_remote_url.as_deref().unwrap_or("<none>"),
            self.post_repo_expected
        );
        println!(
            "Post Repo Private:   {}",
            match self.post_repo_private {
                Some(true) => "YES (verified via gh)",
                Some(false) => "NO (WARNING: REPOSITORY IS PUBLIC!)",
                None => "UNKNOWN (could not verify with gh)",
            }
        );
        println!(
            "Branches Present:    inbox: {}, receipts: {}",
            if self.inbox_branch_exists {
                "YES"
            } else {
                "NO"
            },
            if self.receipts_branch_exists {
                "YES"
            } else {
                "NO"
            }
        );
        println!(
            "Authority Path:      {}",
            if self.authority_path_reachable {
                "YES (reachable and responding)"
            } else {
                "NO (authority check unreachable or failing)"
            }
        );
        println!(
            "Tethers Engine:      {} (path: {})",
            if self.tethers_engine_present {
                "YES (binary present)"
            } else {
                "NO (binary not found)"
            },
            self.tethers_engine_path.as_deref().unwrap_or("<none>")
        );
        println!(
            "Durable Checkpoint:  {}",
            self.durable_checkpoint.as_deref().unwrap_or("<none>")
        );
        println!(
            "Inbox HEAD:          {}",
            self.inbox_head.as_deref().unwrap_or("<none>")
        );
        println!("Pending Intents:     {}", self.pending_intents_count);
        println!(
            "Last Success:        {}",
            self.last_successful_cycle
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "<never>".to_string())
        );
        if let Some(outcome) = &self.last_receipt_outcome {
            println!("Last Receipt:        {outcome}");
        }
        if let Some(err) = &self.last_terminal_error {
            println!("Last Terminal Error: {err}");
        }
        println!(
            "Mirror Freshness:    {}",
            self.mirror_generated_at.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "Mutation Safe:       {}",
            if self.mutation_safe_to_enable {
                "YES"
            } else {
                "NO (one or more invariants not satisfied)"
            }
        );
    }
}
