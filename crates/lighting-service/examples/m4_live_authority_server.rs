//! Local M4 evidence fixture for the authenticated Lantern Warden authority.
//!
//! This example is deliberately not part of the normal Lighting service
//! surface. It seeds one grant through the trusted in-process control seam,
//! then exposes only the existing authenticated Tethers routes over a local
//! TCP listener. No control session or grant bootstrap route is added.

use std::{env, net::SocketAddr};

use axum::serve;
use chrono::{Duration, Utc};
use lighting_core::{AuthorityScope, PrincipalId};
use lighting_service::{AppState, AuthorityService, authority_dto::GrantIntent, build_router};
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealReceiptRepository, SurrealSourceRepository,
    SurrealStore,
};
use tokio::net::TcpListener;

const DEFAULT_BIND: &str = "127.0.0.1:4318";
const DEFAULT_PRINCIPAL: &str = "agent:lucy";
const DEFAULT_CAPABILITY: &str = "demo.export_summary";
const DEFAULT_VERSION: &str = "1";
const DEFAULT_SCOPE_VALUE: &str = "sandbox/Lantern Warden live OpenShell proof";

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::thread::Builder::new()
        .name("lantern-m4-authority".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })?
                .block_on(run())
        })?
        .join()
        .map_err(|_| "Lantern M4 authority thread panicked")?
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = env::var("LANTERN_TETHERS_AUDIT_TOKEN")
        .map_err(|_| "LANTERN_TETHERS_AUDIT_TOKEN is required")?;
    if token.is_empty() {
        return Err("LANTERN_TETHERS_AUDIT_TOKEN cannot be empty".into());
    }

    let bind: SocketAddr = env::var("LANTERN_M4_AUTHORITY_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND.to_owned())
        .parse()?;
    let principal = PrincipalId::new(
        env::var("LANTERN_M4_AUTHORITY_PRINCIPAL").unwrap_or_else(|_| DEFAULT_PRINCIPAL.to_owned()),
    )?;
    let capability_id = env::var("LANTERN_M4_AUTHORITY_CAPABILITY")
        .unwrap_or_else(|_| DEFAULT_CAPABILITY.to_owned());
    let capability_version =
        env::var("LANTERN_M4_AUTHORITY_VERSION").unwrap_or_else(|_| DEFAULT_VERSION.to_owned());
    let scope_value = env::var("LANTERN_M4_AUTHORITY_SCOPE_VALUE")
        .unwrap_or_else(|_| DEFAULT_SCOPE_VALUE.to_owned());

    let store_config = StoreConfig {
        storage: "embedded-surrealkv".to_owned(),
        path: env::var("LANTERN_M4_AUTHORITY_STORE")?.into(),
        endpoint: "ws://127.0.0.1:8000".to_owned(),
        namespace: "lantern_m4".to_owned(),
        database: "authority".to_owned(),
        username: String::new(),
        password: String::new(),
    };
    let store = SurrealStore::connect(&store_config).await?;
    store.initialise_schema().await?;
    SurrealSourceRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(store.clone())
        .migrate()
        .await?;

    let authority = AuthorityService::with_store(store.clone()).with_tethers_audit_token(token);
    authority.migrate().await?;
    SurrealReceiptRepository::new(store).migrate().await?;

    let session = authority
        .open_control_session(PrincipalId::new("human:m4-fixture")?)
        .await;
    let grant = authority
        .issue_from_control_plane(
            &session.session_id,
            &session.csrf_token,
            GrantIntent {
                delegate_principal_id: principal.clone(),
                capability_id,
                capability_version,
                scope: AuthorityScope::from_pairs([
                    ("kind", "path_prefix"),
                    ("value", scope_value.as_str()),
                    ("argument_json_pointer", "/summary"),
                    ("allowed_prefixes", "sandbox/"),
                ])?,
                expires_at: Some(Utc::now() + Duration::hours(1)),
                constraints: Default::default(),
            },
        )
        .await?;

    let listener = TcpListener::bind(bind).await?;
    let mut state = AppState::new_unready();
    state.authority_service = Some(authority);
    state.mark_ready();
    println!(
        "{}",
        serde_json::json!({
            "status": "ready",
            "endpoint": format!("http://{}", listener.local_addr()?),
            "principal_id": principal,
            "grant_id": grant.id,
            "authority_routes": [
                "/api/v1/tethers/authority/check",
                "/api/v1/tethers/receipts"
            ]
        })
    );

    serve(listener, build_router(state)).await?;
    Ok(())
}
