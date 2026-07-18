use std::{env, net::SocketAddr};

use anyhow::{bail, Context};
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{StoreConfig, SurrealStore};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 4317;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let store_config = StoreConfig::from_env();
    let store = SurrealStore::connect(&store_config)
        .await
        .context("failed to connect to SurrealDB")?;
    store
        .initialise_schema()
        .await
        .context("failed to initialise SurrealDB schema")?;

    let address = service_address()?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind Lighting to {address}"))?;
    let app = build_router(AppState::new(store_config));

    info!(%address, "Starting Lighting");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Lighting server failed")?;

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
