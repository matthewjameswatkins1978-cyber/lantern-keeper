//! Live SurrealDB integration tests for the Source HTTP API.
//!
//! These tests validate the full stack: HTTP → service → domain → repository → SurrealDB.
//!
//! Each test uses a unique disposable test database to guarantee isolation.
//!
//! Skip with: `$env:LIGHTING_SKIP_INTEGRATION_TESTS = "1"`

use std::sync::Arc;

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use lighting_service::source_ops::SourceService;
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{StoreConfig, SurrealSourceRepository, SurrealStore};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn skip_integration_tests() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn test_config() -> StoreConfig {
    dotenvy::dotenv().ok();
    let mut config = StoreConfig::from_env();
    config.namespace = "lighting_test".to_owned();
    config.database = format!("lighting_api_test_{}", Uuid::new_v4().simple());
    config
}

async fn connect_repo() -> SurrealSourceRepository {
    let config = test_config();
    let store = SurrealStore::connect(&config)
        .await
        .unwrap_or_else(|error| panic!("failed to connect to local SurrealDB for API integration tests. Start it with `./scripts/start-surreal.ps1`, or set LIGHTING_SKIP_INTEGRATION_TESTS=1 to skip. Error: {error}"));
    let repo = SurrealSourceRepository::new(store);
    repo.migrate()
        .await
        .expect("schema migration should succeed");
    repo
}

fn create_app(repo: Arc<SurrealSourceRepository>) -> Router {
    let source_service = SourceService::new(repo);
    let state = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(source_service),
        project_service: None,
    };
    build_router(state)
}

async fn body_as_value(body: Body) -> Value {
    let bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("body should be readable");
    serde_json::from_slice(&bytes).expect("body should be valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// POST a Markdown Source → GET it → prove exact content round-trip.
#[tokio::test]
async fn store_markdown_and_retrieve_exact_content() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repo().await;
    let repo = Arc::new(repo);
    let app = create_app(repo.clone());

    let content = "# Handbook\n\n  leading\r\n\n\ntrailing  \t";

    // ---- POST /api/v1/sources ----
    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "title": "API test handbook",
                        "kind": "markdown",
                        "content": content
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        create_response.status(),
        StatusCode::CREATED,
        "expected 201 Created for new Source"
    );
    let create_body = body_as_value(create_response.into_body()).await;
    assert_eq!(create_body["outcome"], "stored");
    let source_id = create_body["source_id"].as_str().unwrap().to_owned();
    assert!(!source_id.is_empty());

    // ---- GET /api/v1/sources/{source_id} ----
    let get_response = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/sources/{source_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = body_as_value(get_response.into_body()).await;
    assert_eq!(get_body["source_id"], source_id);
    assert_eq!(get_body["title"], "API test handbook");
    assert_eq!(get_body["kind"], "markdown");
    assert_eq!(get_body["content"], content);
    assert!(!get_body["fingerprint"].as_str().unwrap().is_empty());
    assert!(get_body["created_at"].is_string());
}

/// POST identical content again → HTTP 200 with duplicate outcome and same ID.
#[tokio::test]
async fn duplicate_content_returns_same_id_without_new_record() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repo().await;
    let repo = Arc::new(repo);
    let app = create_app(repo.clone());

    let payload = json!({
        "title": "First",
        "kind": "markdown",
        "content": "duplicate API content"
    });

    // First POST
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = body_as_value(r1.into_body()).await;
    let first_id = b1["source_id"].as_str().unwrap().to_owned();

    // Second POST — same content, different title
    let r2 = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "title": "Second",
                        "kind": "markdown",
                        "content": "duplicate API content"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(r2.status(), StatusCode::OK);
    let b2 = body_as_value(r2.into_body()).await;
    assert_eq!(b2["outcome"], "duplicate");
    assert_eq!(b2["source_id"].as_str().unwrap(), first_id);
}

/// Create a fresh connection to the same database → prove Source survives.
#[tokio::test]
async fn source_survives_fresh_connection() {
    if skip_integration_tests() {
        return;
    }

    // We need to use a shared database name across two connections, so
    // we manually construct the config rather than using test_config().
    dotenvy::dotenv().ok();

    let db_name = format!("lighting_api_reconnect_{}", Uuid::new_v4().simple());

    let mut config_1 = StoreConfig::from_env();
    config_1.namespace = "lighting_test".to_owned();
    config_1.database = db_name.clone();

    let mut config_2 = StoreConfig::from_env();
    config_2.namespace = "lighting_test".to_owned();
    config_2.database = db_name.clone();

    // ---- First connection: store a Source ----
    let store_1 = SurrealStore::connect(&config_1)
        .await
        .expect("first connection should succeed");
    let repo_1 = SurrealSourceRepository::new(store_1);
    repo_1
        .migrate()
        .await
        .expect("first migration should succeed");

    let repo_1 = Arc::new(repo_1);
    let app_1 = create_app(repo_1);

    let content = "persistence check content";

    let create_r = app_1
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "title": "Persistence test",
                        "kind": "plain_text",
                        "content": content
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_r.status(), StatusCode::CREATED);
    let create_body = body_as_value(create_r.into_body()).await;
    let source_id = create_body["source_id"].as_str().unwrap().to_owned();

    // ---- Second connection: retrieve the Source ----
    let store_2 = SurrealStore::connect(&config_2)
        .await
        .expect("second connection should succeed");
    let repo_2 = SurrealSourceRepository::new(store_2);
    repo_2
        .migrate()
        .await
        .expect("second migration should succeed (idempotent)");

    let repo_2 = Arc::new(repo_2);
    let app_2 = create_app(repo_2);

    let get_r = app_2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/sources/{source_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_r.status(), StatusCode::OK);
    let get_body = body_as_value(get_r.into_body()).await;
    assert_eq!(get_body["source_id"], source_id);
    assert_eq!(get_body["content"], content);
}
