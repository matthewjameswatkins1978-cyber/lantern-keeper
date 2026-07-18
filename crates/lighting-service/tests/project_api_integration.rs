//! Live SurrealDB integration tests for the Project HTTP API.

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use lighting_service::project_ops::ProjectService;
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
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
    format!("lighting_project_api_test_{}", Uuid::new_v4().simple())
}

async fn connect_app() -> Router {
    let db = test_db_name();
    let mut config = StoreConfig::from_env();
    config.namespace = "lighting_test".to_owned();
    config.database = db;
    let store = SurrealStore::connect(&config).await.expect("connect");
    let src_repo = SurrealSourceRepository::new(store.clone());
    src_repo.migrate().await.expect("source migration");
    let mp_repo = SurrealMemoryPathRepository::new(store);
    mp_repo.migrate().await.expect("memory-path migration");
    let state = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp_repo))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
    };
    build_router(state)
}

async fn body_as_value(body: Body) -> Value {
    let bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("body readable");
    serde_json::from_slice(&bytes).expect("valid JSON")
}

#[tokio::test]
async fn create_then_get_project_through_http() {
    if skip_integration_tests() {
        return;
    }
    dotenvy::dotenv().ok();
    let app = connect_app().await;
    let post_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"name":"Lantern Keeper","status":"active"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_resp.status(), StatusCode::CREATED);
    let post_body = body_as_value(post_resp.into_body()).await;
    let pid = post_body["project_id"].as_str().unwrap().to_owned();
    let get_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = body_as_value(get_resp.into_body()).await;
    assert_eq!(get_body["project_id"], pid);
    assert_eq!(get_body["name"], "Lantern Keeper");
    assert_eq!(get_body["status"], "active");
}

#[tokio::test]
async fn paused_status_round_trip() {
    if skip_integration_tests() {
        return;
    }
    dotenvy::dotenv().ok();
    let app = connect_app().await;
    let post_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"name":"Paused example","status":"paused"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_resp.status(), StatusCode::CREATED);
    let post_body = body_as_value(post_resp.into_body()).await;
    assert_eq!(post_body["status"], "paused");
    let pid = post_body["project_id"].as_str().unwrap();
    let get_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = body_as_value(get_resp.into_body()).await;
    assert_eq!(get_body["status"], "paused");
}

#[tokio::test]
async fn project_survives_fresh_connection() {
    if skip_integration_tests() {
        return;
    }
    dotenvy::dotenv().ok();
    let db_name = test_db_name();
    let mut config_1 = StoreConfig::from_env();
    config_1.namespace = "lighting_test".to_owned();
    config_1.database = db_name.clone();
    let store_1 = SurrealStore::connect(&config_1).await.expect("connect");
    let mp_1 = SurrealMemoryPathRepository::new(store_1);
    mp_1.migrate().await.expect("migrate");
    let app_1 = build_router(AppState {
        ready: Arc::new(true.into()),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp_1))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
    });
    let post_resp = app_1
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name":"Persistence test"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_resp.status(), StatusCode::CREATED);
    let post_body = body_as_value(post_resp.into_body()).await;
    let pid = post_body["project_id"].as_str().unwrap().to_owned();
    let mut config_2 = StoreConfig::from_env();
    config_2.namespace = "lighting_test".to_owned();
    config_2.database = db_name;
    let store_2 = SurrealStore::connect(&config_2).await.expect("connect");
    let mp_2 = SurrealMemoryPathRepository::new(store_2);
    mp_2.migrate().await.expect("migrate");
    let app_2 = build_router(AppState {
        ready: Arc::new(true.into()),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp_2))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
    });
    let get_resp = app_2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = body_as_value(get_resp.into_body()).await;
    assert_eq!(get_body["project_id"], pid);
    assert_eq!(get_body["name"], "Persistence test");
    assert_eq!(get_body["status"], "active");
}
