//! Live SurrealDB integration tests for the Episode HTTP API.

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use lighting_service::episode_ops::EpisodeService;
use lighting_service::source_ops::SourceService;
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
    format!("lighting_episode_api_test_{}", Uuid::new_v4().simple())
}

async fn connect_app() -> Router {
    dotenvy::dotenv().ok();
    let db = test_db_name();
    let mut c = StoreConfig::from_env();
    c.namespace = "lighting_test".to_owned();
    c.database = db;
    let store = SurrealStore::connect(&c).await.expect("connect");
    let src = SurrealSourceRepository::new(store.clone());
    src.migrate().await.expect("src migrate");
    let mp = SurrealMemoryPathRepository::new(store);
    mp.migrate().await.expect("mp migrate");
    let src: Arc<dyn lighting_core::SourceRepository> = Arc::new(src);
    let mp: Arc<dyn lighting_core::MemoryPathRepository> = Arc::new(mp);
    let state = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&src))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&src), Arc::clone(&mp))),
        association_service: None,
        retrieval_service: None,
    };
    build_router(state)
}
async fn body_as_value(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("readable");
    serde_json::from_slice(&b).expect("valid JSON")
}
async fn create_source(app: &Router, content: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Test","kind":"plain_text","content":content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    body_as_value(r.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn create_then_get_exact_episode_excerpt() {
    if skip_integration_tests() {
        return;
    }
    let app = connect_app().await;
    let content = "Line zero\nLine one\nLine two\nLine three\nLine four\n";
    let source_id = create_source(&app, content).await;
    let start_byte: usize = 15;
    let end_byte: usize = 28;
    let expected = &content[start_byte..end_byte];
    let post_resp = app.clone().oneshot(Request::builder().method(http::Method::POST).uri("/api/v1/episodes").header(http::header::CONTENT_TYPE, "application/json").body(Body::from(json!({"title":"The Human Relay Problem","source_id":source_id,"start_byte":start_byte,"end_byte":end_byte}).to_string())).unwrap()).await.unwrap();
    assert_eq!(post_resp.status(), StatusCode::CREATED);
    let pb = body_as_value(post_resp.into_body()).await;
    assert_eq!(pb["excerpt"], expected);
    let eid = pb["episode_id"].as_str().unwrap().to_owned();
    let get_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let gb = body_as_value(get_resp.into_body()).await;
    assert_eq!(gb["excerpt"], expected);
}

#[tokio::test]
async fn utf8_boundary_rejection_returns_400() {
    if skip_integration_tests() {
        return;
    }
    let app = connect_app().await;
    let source_id = create_source(&app, "fooébar").await;
    let post_resp = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/episodes")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Bad","source_id":source_id,"start_byte":3,"end_byte":4})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_as_value(post_resp.into_body()).await["code"],
        "invalid_episode_range"
    );
}

#[tokio::test]
async fn episode_survives_fresh_connection_with_exact_excerpt() {
    if skip_integration_tests() {
        return;
    }
    dotenvy::dotenv().ok();
    let db_name = test_db_name();
    let mut c1 = StoreConfig::from_env();
    c1.namespace = "lighting_test".to_owned();
    c1.database = db_name.clone();
    let s1 = SurrealStore::connect(&c1).await.expect("connect");
    let src1 = SurrealSourceRepository::new(s1.clone());
    src1.migrate().await.expect("migrate");
    let mp1 = SurrealMemoryPathRepository::new(s1);
    mp1.migrate().await.expect("migrate");
    let ar1: Arc<dyn lighting_core::SourceRepository> = Arc::new(src1);
    let app1 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&ar1))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&ar1), Arc::new(mp1))),
        association_service: None,
        retrieval_service: None,
    });
    let content = "Persistence test content line\nSecond line here\n";
    let source_id = create_source(&app1, content).await;
    let start_byte: usize = 0;
    let end_byte: usize = 31;
    let expected = &content[start_byte..end_byte];
    let post = app1.oneshot(Request::builder().method(http::Method::POST).uri("/api/v1/episodes").header(http::header::CONTENT_TYPE, "application/json").body(Body::from(json!({"title":"P","source_id":source_id,"start_byte":start_byte,"end_byte":end_byte}).to_string())).unwrap()).await.unwrap();
    assert_eq!(post.status(), StatusCode::CREATED);
    let eid = body_as_value(post.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut c2 = StoreConfig::from_env();
    c2.namespace = "lighting_test".to_owned();
    c2.database = db_name;
    let s2 = SurrealStore::connect(&c2).await.expect("connect");
    let src2 = SurrealSourceRepository::new(s2.clone());
    src2.migrate().await.expect("migrate");
    let mp2 = SurrealMemoryPathRepository::new(s2);
    mp2.migrate().await.expect("migrate");
    let ar2: Arc<dyn lighting_core::SourceRepository> = Arc::new(src2);
    let app2 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&ar2))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&ar2), Arc::new(mp2))),
        association_service: None,
        retrieval_service: None,
    });
    let get = app2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(body_as_value(get.into_body()).await["excerpt"], expected);
}
