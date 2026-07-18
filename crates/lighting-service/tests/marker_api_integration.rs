//! Live SurrealDB integration tests for the Marker HTTP API.

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use lighting_service::marker_ops::MarkerService;
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{StoreConfig, SurrealMemoryPathRepository, SurrealStore};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

fn skip_integration_tests() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn test_db_name() -> String {
    format!("lighting_marker_api_test_{}", Uuid::new_v4().simple())
}

async fn connect_app() -> Router {
    dotenvy::dotenv().ok();
    let db = test_db_name();
    let mut config = StoreConfig::from_env();
    config.namespace = "lighting_test".to_owned();
    config.database = db;
    let store = SurrealStore::connect(&config).await.expect("connect");
    let mp_repo = SurrealMemoryPathRepository::new(store);
    mp_repo.migrate().await.expect("migrate");
    let state = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: None,
        marker_service: Some(MarkerService::new(Arc::new(mp_repo))),
        episode_service: None,
        association_service: None,
        retrieval_service: None,
    };
    build_router(state)
}

async fn body_as_value(body: Body) -> Value {
    let bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("readable");
    serde_json::from_slice(&bytes).expect("valid JSON")
}

#[tokio::test]
async fn create_then_get_marker_through_http() {
    if skip_integration_tests() {
        return;
    }
    let app = connect_app().await;
    let post_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"human network cable"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_resp.status(), StatusCode::CREATED);
    let post_body = body_as_value(post_resp.into_body()).await;
    let mid = post_body["marker_id"].as_str().unwrap().to_owned();
    let get_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/markers/{mid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = body_as_value(get_resp.into_body()).await;
    assert_eq!(get_body["marker_id"], mid);
    assert_eq!(get_body["display_text"], "human network cable");
    assert_eq!(get_body["lookup_key"], "human network cable");
}

#[tokio::test]
async fn normalised_duplicate_returns_200_with_original_id() {
    if skip_integration_tests() {
        return;
    }
    let app = connect_app().await;
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"human network cable"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = body_as_value(r1.into_body()).await;
    let original_id = b1["marker_id"].as_str().unwrap().to_owned();
    let r2 = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"  HUMAN   Network Cable  "}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let b2 = body_as_value(r2.into_body()).await;
    assert_eq!(b2["marker_id"], original_id);
}

#[tokio::test]
async fn lookup_via_different_casing_whitespace_finds_original() {
    if skip_integration_tests() {
        return;
    }
    let app = connect_app().await;
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"human network cable"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = body_as_value(r1.into_body()).await;
    let original_id = b1["marker_id"].as_str().unwrap().to_owned();
    let lookup_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/markers/lookup?text=HUMAN+++network++cable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(lookup_resp.status(), StatusCode::OK);
    let lookup_body = body_as_value(lookup_resp.into_body()).await;
    assert_eq!(lookup_body["marker_id"], original_id);
}
