use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use super::schema::Receipt;

pub const EXPECTED_POST_REPO: &str = "matthewjameswatkins1978-cyber/lantern-post";
pub const INBOX_BRANCH: &str = "inbox";
pub const RECEIPTS_BRANCH: &str = "receipts";

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("git command failed ({cmd}): {output}")]
    GitCommand { cmd: String, output: String },
    #[error("remote origin is unexpected: '{0}' (expected '{EXPECTED_POST_REPO}')")]
    UnexpectedRemote(String),
    #[error("repository visibility is not private: {0}")]
    PublicRepositoryForbidden(String),
    #[error(
        "SUSPECT: inbox HEAD '{inbox_head}' is not a descendant of checkpoint '{checkpoint}'. Queue history was rewritten!"
    )]
    NonDescendantInboxHead {
        checkpoint: String,
        inbox_head: String,
    },
    #[error(
        "commit '{commit}' modified or deleted existing intent file: '{path}'. Intents must be append-only!"
    )]
    ForbiddenModification { commit: String, path: String },
    #[error("I/O error during queue operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredIntentCommit {
    pub commit_sha: String,
    pub intent_file_path: String,
    pub intent_raw_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BridgeStatusFile {
    pub bridge_version: String,
    pub last_cycle_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_processed_inbox_commit: Option<String>,
    pub inbox_head: Option<String>,
    pub pending_count: usize,
    pub last_terminal_error: Option<String>,
    pub lantern_reachable: bool,
    pub tethers_reachable: bool,
    pub mirror_last_generated_at: Option<String>,
}

pub struct GitQueue {
    repo_path: PathBuf,
}

impl GitQueue {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    fn run_git(&self, args: &[&str]) -> Result<String, QueueError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(args)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let msg = if !stderr.is_empty() { stderr } else { stdout };
            return Err(QueueError::GitCommand {
                cmd: format!("git {}", args.join(" ")),
                output: msg,
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn verify_repository(&self, check_gh_privacy: bool) -> Result<(), QueueError> {
        let remote_url = self.run_git(&["remote", "get-url", "origin"])?;
        if !remote_url.contains(EXPECTED_POST_REPO) {
            return Err(QueueError::UnexpectedRemote(remote_url));
        }

        if check_gh_privacy {
            let gh_output = Command::new("gh")
                .args([
                    "repo",
                    "view",
                    EXPECTED_POST_REPO,
                    "--json",
                    "isPrivate",
                    "--jq",
                    ".isPrivate",
                ])
                .output();

            match gh_output {
                Ok(out) if out.status.success() => {
                    let is_private = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if is_private != "true" {
                        return Err(QueueError::PublicRepositoryForbidden(format!(
                            "gh reported isPrivate={is_private}"
                        )));
                    }
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    return Err(QueueError::PublicRepositoryForbidden(format!(
                        "gh repo view failed: {err}"
                    )));
                }
                Err(e) => {
                    return Err(QueueError::PublicRepositoryForbidden(format!(
                        "could not verify privacy with gh: {e}"
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn fetch_remote(&self) -> Result<(), QueueError> {
        self.run_git(&["fetch", "origin"])?;
        Ok(())
    }

    pub fn get_inbox_head(&self) -> Result<String, QueueError> {
        // Try origin/inbox first, then local inbox
        if let Ok(head) = self.run_git(&["rev-parse", "origin/inbox"])
            && !head.is_empty()
        {
            return Ok(head);
        }
        self.run_git(&["rev-parse", "inbox"])
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, QueueError> {
        if ancestor == descendant {
            return Ok(true);
        }
        let status = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .status()?;
        Ok(status.success())
    }

    pub fn list_commits_between(
        &self,
        checkpoint: Option<&str>,
        inbox_head: &str,
    ) -> Result<Vec<String>, QueueError> {
        if let Some(cp) = checkpoint {
            if cp == inbox_head {
                return Ok(Vec::new());
            }
            if !self.is_ancestor(cp, inbox_head)? {
                return Err(QueueError::NonDescendantInboxHead {
                    checkpoint: cp.to_string(),
                    inbox_head: inbox_head.to_string(),
                });
            }
            let output =
                self.run_git(&["rev-list", "--reverse", &format!("{cp}..{inbox_head}")])?;
            let commits = output
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            Ok(commits)
        } else {
            // First time: process all commits on inbox up to inbox_head
            let output = self.run_git(&["rev-list", "--reverse", inbox_head])?;
            let commits = output
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            Ok(commits)
        }
    }

    pub fn extract_intent_from_commit(
        &self,
        commit_sha: &str,
    ) -> Result<Option<DiscoveredIntentCommit>, QueueError> {
        let diff_output = self.run_git(&[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            commit_sha,
        ])?;

        let mut intent_file = None;

        for line in diff_output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let status = parts.next().unwrap_or("");
            let file_path = parts.next().unwrap_or("");

            if file_path.starts_with("intents/") && file_path.ends_with(".json") {
                if status.starts_with('M') || status.starts_with('D') {
                    return Err(QueueError::ForbiddenModification {
                        commit: commit_sha.to_string(),
                        path: file_path.to_string(),
                    });
                }
                if status.starts_with('A') {
                    intent_file = Some(file_path.to_string());
                }
            }
        }

        if let Some(path) = intent_file {
            let content = self.run_git(&["show", &format!("{commit_sha}:{path}")])?;
            Ok(Some(DiscoveredIntentCommit {
                commit_sha: commit_sha.to_string(),
                intent_file_path: path,
                intent_raw_json: content,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn write_receipts_and_status(
        &self,
        receipts: &[Receipt],
        status_file: &BridgeStatusFile,
        push: bool,
    ) -> Result<Option<String>, QueueError> {
        if receipts.is_empty() && status_file.last_processed_inbox_commit.is_none() {
            return Ok(None);
        }

        // Checkout local receipts branch
        self.run_git(&["checkout", RECEIPTS_BRANCH])?;
        // If tracking remote receipts, try fast-forward pull
        let _ = self.run_git(&["pull", "--ff-only", "origin", RECEIPTS_BRANCH]);

        let receipts_dir = self.repo_path.join("receipts");
        std::fs::create_dir_all(&receipts_dir)?;
        let status_dir = self.repo_path.join("status");
        std::fs::create_dir_all(&status_dir)?;

        for receipt in receipts {
            let file_name = format!("{}.json", receipt.intent_id);
            let target = receipts_dir.join(file_name);
            let json = serde_json::to_string_pretty(receipt)?;
            std::fs::write(target, json + "\n")?;
        }

        let status_json = serde_json::to_string_pretty(status_file)?;
        std::fs::write(status_dir.join("bridge.json"), status_json + "\n")?;

        self.run_git(&["add", "receipts", "status/bridge.json"])?;

        // Check if there are changes to commit
        let status = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["diff", "--cached", "--quiet"])
            .status()?;

        if status.success() {
            // No changes
            return Ok(None);
        }

        let commit_msg = if receipts.len() == 1 {
            format!("Bridge receipt for {}", receipts[0].intent_id)
        } else if receipts.is_empty() {
            "Update bridge status".to_string()
        } else {
            format!("Bridge receipts for {} intents", receipts.len())
        };

        self.run_git(&["commit", "-m", &commit_msg])?;
        let receipt_commit = self.run_git(&["rev-parse", "HEAD"])?;

        if push {
            self.run_git(&["push", "origin", RECEIPTS_BRANCH])?;
        }

        Ok(Some(receipt_commit))
    }
}
