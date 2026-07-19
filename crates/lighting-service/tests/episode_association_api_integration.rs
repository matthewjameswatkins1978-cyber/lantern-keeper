//! Live SurrealDB integration tests for Episode association HTTP endpoints.

use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use lighting_service::episode_ops::EpisodeService;
use lighting_service::marker_ops::MarkerService;
use lighting_service::source_ops::SourceService;
use lighting_service::{build_router, AppState, EpisodeAssociationService, ProjectService};
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

fn skip() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}
fn db_name() -> String {
    format!("laat_{}", Uuid::new_v4().simple())
}

async fn full_app() -> Router {
    dotenvy::dotenv().ok();
    let db = db_name();
    let mut c = StoreConfig::from_env();
    c.namespace = "lighting_test".to_owned();
    c.database = db;
    let s = SurrealStore::connect(&c).await.expect("connect");
    let src = SurrealSourceRepository::new(s.clone());
    src.migrate().await.expect("migrate");
    let mp = SurrealMemoryPathRepository::new(s);
    mp.migrate().await.expect("migrate");
    let sr: Arc<dyn lighting_core::SourceRepository> = Arc::new(src);
    let mr: Arc<dyn lighting_core::MemoryPathRepository> = Arc::new(mp);
    build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&sr))),
        project_service: Some(ProjectService::new(Arc::clone(&mr), Arc::clone(&sr))),
        marker_service: Some(MarkerService::new(Arc::clone(&mr))),
        episode_service: Some(EpisodeService::new(Arc::clone(&sr), Arc::clone(&mr))),
        association_service: Some(EpisodeAssociationService::new(Arc::clone(&mr))),
        retrieval_service: None,
        project_retrieval_service: None,
    })
}
async fn body_json(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("readable");
    serde_json::from_slice(&b).expect("json")
}
async fn create_source(app: &Router) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"T","kind":"plain_text","content":"line one\nline two\n"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    body_json(r.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn create_episode(app: &Router, sid: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/episodes")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"E","source_id":sid,"start_byte":0,"end_byte":9}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    body_json(r.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn create_project(app: &Router, name: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"name":name,"status":"active"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    body_json(r.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn create_marker(app: &Router, text: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"text":text}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    body_json(r.into_body()).await["marker_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn episode_project_link_idempotent_and_lists_one() {
    if skip() {
        return;
    }
    let app = full_app().await;
    let sid = create_source(&app).await;
    let eid = create_episode(&app, &sid).await;
    let pid = create_project(&app, "P").await;
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid}/projects"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":pid,"kind":"primary"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::NO_CONTENT);
    let r2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid}/projects"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":pid,"kind":"primary"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::NO_CONTENT);
    let list = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}/projects"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(list.into_body()).await;
    assert_eq!(body["projects"].as_array().unwrap().len(), 1);
    assert_eq!(body["projects"][0]["kind"], "primary");
}

#[tokio::test]
async fn episode_marker_link_idempotent_and_lists_one() {
    if skip() {
        return;
    }
    let app = full_app().await;
    let sid = create_source(&app).await;
    let eid = create_episode(&app, &sid).await;
    let mid = create_marker(&app, "test marker").await;
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid}/markers"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"marker_id":mid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::NO_CONTENT);
    let r2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid}/markers"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"marker_id":mid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::NO_CONTENT);
    let list = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}/markers"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        body_json(list.into_body()).await["markers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn link_missing_target_returns_404() {
    if skip() {
        return;
    }
    let app = full_app().await;
    let sid = create_source(&app).await;
    let eid = create_episode(&app, &sid).await;
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid}/projects"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":"00000000-0000-0000-0000-000000000000","kind":"primary"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
