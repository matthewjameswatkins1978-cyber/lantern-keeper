pub mod doctor;
pub mod executor;
pub mod queue;
pub mod runner;
pub mod schema;
pub mod state;
pub mod tethers;

#[cfg(test)]
mod tests;

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum BridgeGithubSubcommand {
    /// Perform one complete poll/process/receipt cycle and exit.
    Once {
        /// Path to lantern-post repository clone.
        #[arg(
            long,
            env = "LANTERN_POST_REPO_PATH",
            default_value = "D:\\Projects\\lantern-post"
        )]
        post_repo: PathBuf,

        /// Path to lantern-git repository clone (for mirror refresh).
        #[arg(
            long,
            env = "LANTERN_GIT_REPO_PATH",
            default_value = "D:\\Projects\\lantern-git"
        )]
        git_repo: Option<PathBuf>,

        /// Path to durable state file.
        #[arg(
            long,
            env = "LANTERN_BRIDGE_STATE_PATH",
            default_value = ".lighting-runtime\\bridge-state.json"
        )]
        state_path: PathBuf,

        /// Push receipts to remote repository (disable for offline/local tests).
        #[arg(long, default_value_t = true)]
        push: bool,
    },
    /// Run bridge continuously in background polling mode with backoff.
    Run {
        /// Path to lantern-post repository clone.
        #[arg(
            long,
            env = "LANTERN_POST_REPO_PATH",
            default_value = "D:\\Projects\\lantern-post"
        )]
        post_repo: PathBuf,

        /// Path to lantern-git repository clone (for mirror refresh).
        #[arg(
            long,
            env = "LANTERN_GIT_REPO_PATH",
            default_value = "D:\\Projects\\lantern-git"
        )]
        git_repo: Option<PathBuf>,

        /// Path to durable state file.
        #[arg(
            long,
            env = "LANTERN_BRIDGE_STATE_PATH",
            default_value = ".lighting-runtime\\bridge-state.json"
        )]
        state_path: PathBuf,

        /// Normal polling interval in seconds.
        #[arg(long, default_value_t = 30)]
        poll_interval_secs: u64,

        /// Push receipts to remote repository.
        #[arg(long, default_value_t = true)]
        push: bool,
    },
    /// Report actionable diagnostics on bridge state, remotes, and authority.
    Doctor {
        /// Path to lantern-post repository clone.
        #[arg(
            long,
            env = "LANTERN_POST_REPO_PATH",
            default_value = "D:\\Projects\\lantern-post"
        )]
        post_repo: PathBuf,

        /// Path to lantern-git repository clone.
        #[arg(
            long,
            env = "LANTERN_GIT_REPO_PATH",
            default_value = "D:\\Projects\\lantern-git"
        )]
        git_repo: Option<PathBuf>,

        /// Path to durable state file.
        #[arg(
            long,
            env = "LANTERN_BRIDGE_STATE_PATH",
            default_value = ".lighting-runtime\\bridge-state.json"
        )]
        state_path: PathBuf,

        /// Output diagnostic report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum BridgeSubcommand {
    /// GitHub transport bridge operations.
    #[command(subcommand)]
    Github(BridgeGithubSubcommand),
}
