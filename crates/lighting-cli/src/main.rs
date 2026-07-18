use anyhow::Context;
use clap::{Parser, Subcommand};
use lighting_store_surreal::{StoreConfig, SurrealStore};

#[derive(Debug, Parser)]
#[command(name = "lighting-cli", about = "Development commands for Lighting")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Health,
    InitDb,
    PrintConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = StoreConfig::from_env();

    match cli.command {
        Command::Health => {
            let store = SurrealStore::connect(&config)
                .await
                .context("failed to connect to SurrealDB")?;
            store.health_check().await.context("health check failed")?;
            println!("Lighting database health check succeeded");
        }
        Command::InitDb => {
            let store = SurrealStore::connect(&config)
                .await
                .context("failed to connect to SurrealDB")?;
            store
                .initialise_schema()
                .await
                .context("schema initialisation failed")?;
            println!("Lighting schema initialisation completed");
        }
        Command::PrintConfig => {
            let json = serde_json::to_string_pretty(&config.redacted())
                .context("failed to render redacted configuration")?;
            println!("{json}");
        }
    }

    Ok(())
}
