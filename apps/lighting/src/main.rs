use std::{env, net::SocketAddr, sync::Arc};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use lighting_cli::{CliCommand, default_service_url, run_cli_command, validate_service_url};
use lighting_service::source_ops::SourceService;
use lighting_service::tethers_engine_client::{TethersEngineClient, TethersEngineError};
use lighting_service::{
    AppState, EpisodeAssociationService, EpisodeService, MarkerRetrievalService, MarkerService,
    MemoryService, ProjectRetrievalService, ProjectService, build_router,
};
use lighting_store_surreal::{
    ExportSummary, StoreConfig, SurrealMemoryPathRepository, SurrealMemoryRepository,
    SurrealSourceRepository, SurrealStore,
};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 4317;

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve => {
            init_tracing();
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("failed to create tokio runtime")?;
            rt.block_on(async {
                tokio::spawn(serve())
                    .await
                    .context("Lighting service task panicked")?
            })
        }
        Command::Version => {
            println!("Lighting {} (Lantern Keeper)", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Health { json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::Health { json })
        }
        Command::SourceAdd {
            path,
            title,
            kind,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::SourceAdd {
                    path,
                    title,
                    kind,
                    json,
                },
            )
        }
        Command::SourceShow { source_id, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::SourceShow { source_id, json })
        }
        Command::SourceHistory { path, kind, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::SourceHistory { path, kind, json })
        }
        Command::Retrieve { phrase, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::Retrieve { phrase, json })
        }
        Command::Remember {
            content,
            kind,
            project_id,
            confidence,
            importance,
            derived_from,
            supersedes,
            contradicts,
            supports,
            agent,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::Remember {
                    content,
                    kind,
                    project_id,
                    confidence,
                    importance,
                    derived_from,
                    supersedes,
                    contradicts,
                    supports,
                    agent,
                    json,
                },
            )
        }
        Command::Recall {
            project_id,
            phrase,
            include_inactive,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::Recall {
                    project_id,
                    phrase,
                    include_inactive,
                    json,
                },
            )
        }
        Command::Context {
            project_id,
            query,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::Context {
                    project_id,
                    query,
                    json,
                },
            )
        }
        Command::MemorySupersede { memory_id, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::MemorySupersede { memory_id, json })
        }
        Command::Export { output } => {
            init_tracing();
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("failed to create tokio runtime")?;
            rt.block_on(async {
                tokio::spawn(export_data(output))
                    .await
                    .context("Lighting export task panicked")?
            })
        }
        Command::ProjectHandoff { project_id, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::ProjectHandoff { project_id, json })
        }
        Command::ProjectRecordResult {
            project_id,
            path,
            title,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::ProjectRecordResult {
                    project_id,
                    path,
                    title,
                    json,
                },
            )
        }
        Command::ProjectCreate { name, json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::ProjectCreate { name, json })
        }
        Command::ProjectList { json } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(&url, CliCommand::ProjectList { json })
        }
        Command::ProjectAddFile {
            project_id,
            path,
            title,
            kind,
            json,
        } => {
            let url = cli.service_url.unwrap_or_else(default_service_url);
            run_cli_command(
                &url,
                CliCommand::ProjectAddFile {
                    project_id,
                    path,
                    title,
                    kind,
                    json,
                },
            )
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "lighting", about = "Lantern Keeper's Lighting")]
struct Cli {
    /// Lighting service URL (for CLI commands that talk to the service).
    #[arg(long, env = "LIGHTING_SERVICE_URL", value_parser = validate_service_url)]
    service_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the local Lighting HTTP service.
    Serve,
    /// Print the Lighting version.
    Version,
    /// Check Lighting service readiness.
    Health {
        #[arg(long)]
        json: bool,
    },
    /// Add a file as a new Source.
    SourceAdd {
        /// Path to the file.
        path: std::path::PathBuf,
        /// Source title (default: normalised absolute path).
        #[arg(long)]
        title: Option<String>,
        /// Source kind (default: inferred from extension).
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Show a Source by ID.
    SourceShow {
        /// Source ID (UUID).
        source_id: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Show revision history for a file-backed Source.
    SourceHistory {
        /// Path to the file.
        path: std::path::PathBuf,
        /// Source kind (default: inferred from extension).
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Retrieve Episodes by a remembered Marker phrase.
    Retrieve {
        /// The remembered phrase to look up.
        phrase: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Record a derived, provenance-bearing Memory.
    Remember {
        content: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long, default_value_t = 0.8)]
        confidence: f32,
        #[arg(long, default_value_t = 0.5)]
        importance: f32,
        #[arg(long)]
        derived_from: Vec<String>,
        #[arg(long)]
        supersedes: Vec<String>,
        #[arg(long)]
        contradicts: Vec<String>,
        #[arg(long)]
        supports: Vec<String>,
        #[arg(long, default_value = "lucy")]
        agent: String,
        #[arg(long)]
        json: bool,
    },
    /// Recall active Memory records.
    Recall {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        phrase: Option<String>,
        #[arg(long)]
        include_inactive: bool,
        #[arg(long)]
        json: bool,
    },
    /// Build a compact working context from active Memory records.
    Context {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Mark a Memory as superseded while retaining its history.
    MemorySupersede {
        memory_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Export logical records to an engine-independent backup directory.
    Export {
        /// Destination directory for manifest.json and NDJSON files.
        output: std::path::PathBuf,
    },
    /// Produce a Codex-ready handoff for a Project.
    ProjectHandoff {
        /// Project ID (UUID).
        project_id: String,
        /// Output the full JSON response only.
        #[arg(long)]
        json: bool,
    },
    /// Record a result file against a Project.
    ProjectRecordResult {
        /// Project ID (UUID).
        project_id: String,
        /// Path to the result file.
        path: std::path::PathBuf,
        /// Result title (default: file name).
        #[arg(long)]
        title: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Create a new Project.
    ProjectCreate {
        /// Project name.
        name: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// List all Projects.
    ProjectList {
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Add a local file to a Project's handoff context.
    ProjectAddFile {
        /// Project ID (UUID).
        project_id: String,
        /// Path to the file.
        path: std::path::PathBuf,
        /// Source title (default: normalised absolute path).
        #[arg(long)]
        title: Option<String>,
        /// Source kind (default: inferred from extension).
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
}

async fn serve() -> anyhow::Result<()> {
    let address = service_address()?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind Lighting to {address}"))?;

    let store_config = StoreConfig::from_env();
    info!(
        config = %serde_json::to_string(&store_config.redacted()).unwrap_or_else(|_| "<redacted>".into()),
        "Connecting to SurrealDB"
    );

    let store = SurrealStore::connect(&store_config)
        .await
        .context("failed to connect to SurrealDB. Ensure it is running on the configured endpoint and the credentials are correct.")?;

    store
        .health_check()
        .await
        .context("SurrealDB health check failed after connecting")?;

    let repo = SurrealSourceRepository::new(store.clone());

    repo.migrate()
        .await
        .context("failed to apply Source schema migration. Verify that the SurrealDB user has permissions to define tables and indexes.")?;

    info!("SurrealDB Source schema migration applied successfully");

    let mp_repo = SurrealMemoryPathRepository::new(store.clone());
    mp_repo
        .migrate()
        .await
        .context("failed to apply memory-path schema migrations. Verify that the SurrealDB user has permissions to define tables and indexes.")?;

    info!("Memory-path schema migrations applied successfully");

    let memory_repo = SurrealMemoryRepository::new(store.clone());
    memory_repo
        .migrate()
        .await
        .context("failed to apply Living Memory schema migration")?;

    info!("Living Memory schema migration applied successfully");

    let source_repo: Arc<dyn lighting_core::SourceRepository> = Arc::new(repo);
    let mp_repo: Arc<dyn lighting_core::MemoryPathRepository> = Arc::new(mp_repo);
    let memory_repo: Arc<dyn lighting_core::MemoryRepository> = Arc::new(memory_repo);
    let source_service = SourceService::new(Arc::clone(&source_repo));
    let project_service = ProjectService::new(Arc::clone(&mp_repo), Arc::clone(&source_repo));
    let marker_service = MarkerService::new(Arc::clone(&mp_repo));
    let episode_service = EpisodeService::new(Arc::clone(&source_repo), Arc::clone(&mp_repo));
    let association_service = EpisodeAssociationService::new(Arc::clone(&mp_repo));
    let retrieval_service =
        MarkerRetrievalService::new(Arc::clone(&mp_repo), Arc::clone(&source_repo));
    let project_retrieval_service =
        ProjectRetrievalService::new(Arc::clone(&mp_repo), Arc::clone(&source_repo));
    let memory_service = MemoryService::new(Arc::clone(&memory_repo));
    let tethers_client = match TethersEngineClient::from_env() {
        Ok(client) => Some(client),
        Err(TethersEngineError::MissingEnginePath) => None,
        Err(error) => {
            return Err(error).context("failed to configure Tethers engine client");
        }
    };
    let app_state = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        source_service: Some(source_service),
        project_service: Some(project_service),
        marker_service: Some(marker_service),
        episode_service: Some(episode_service),
        association_service: Some(association_service),
        retrieval_service: Some(retrieval_service),
        project_retrieval_service: Some(project_retrieval_service),
        tethers_client,
        memory_service: Some(memory_service),
    };
    app_state.mark_ready();

    info!(%address, "Starting Lighting — durable Source storage is ready");

    let app = build_router(app_state);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Lighting server failed")?;

    Ok(())
}

async fn export_data(output: std::path::PathBuf) -> anyhow::Result<()> {
    let store_config = StoreConfig::from_env();
    let store = SurrealStore::connect(&store_config)
        .await
        .context("failed to connect to the configured Lantern store")?;
    store
        .initialise_schema()
        .await
        .context("failed to initialise the base Lantern schema")?;
    SurrealSourceRepository::new(store.clone())
        .migrate()
        .await
        .context("failed to initialise the Source schema")?;
    SurrealMemoryPathRepository::new(store.clone())
        .migrate()
        .await
        .context("failed to initialise the project/memory-path schema")?;
    SurrealMemoryRepository::new(store.clone())
        .migrate()
        .await
        .context("failed to initialise the Living Memory schema")?;
    let summary: ExportSummary = store
        .export_to(&output)
        .await
        .context("failed to export Lantern records")?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn service_address() -> anyhow::Result<SocketAddr> {
    let host = env::var("LIGHTING_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_owned());
    let port = match env::var("LIGHTING_PORT") {
        Ok(value) => value
            .parse::<u16>()
            .with_context(|| format!("LIGHTING_PORT must be a valid TCP port, got {value}"))?,
        Err(_) => DEFAULT_PORT,
    };

    if host != "127.0.0.1" && host != "localhost" {
        bail!("Lighting currently supports localhost-only binding; set LIGHTING_HOST=127.0.0.1");
    }

    format!("{host}:{port}")
        .parse()
        .with_context(|| format!("failed to parse Lighting bind address {host}:{port}"))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "Failed to listen for shutdown signal");
    }
}
