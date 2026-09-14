//! Standalone `lighting-cli` binary — thin shim over the CLI library.
//!
//! The primary user-facing command is `lighting` (in `apps/lighting`).
//! This binary exists so the CLI tests remain self-contained.

use clap::Parser;
use lighting_cli::{CliCommand, default_service_url, run_cli_command, validate_service_url};

#[derive(Debug, Parser)]
#[command(name = "lighting-cli", about = "Lighting CLI (standalone)")]
struct Cli {
    #[arg(long, env = "LIGHTING_SERVICE_URL", default_value_t = default_service_url(), value_parser = validate_service_url)]
    service_url: String,

    #[command(subcommand)]
    command: CliCommand,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run_cli_command(&cli.service_url, cli.command)
}
