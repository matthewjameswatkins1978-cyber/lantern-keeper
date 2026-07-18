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
fn skip() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}
fn db() -> String {
    format!("lp_{}", Uuid::new_v4().simple())
}
async fn app() -> Router {
    let d = db();
    let mut c = StoreConfig::from_env();
    c.namespace = "lighting_test".to_owned();
    c.database = d;
    let s = SurrealStore::connect(&c).await.expect("c");
    SurrealSourceRepository::new(s.clone())
        .migrate()
        .await
        .expect("m");
    let mp = SurrealMemoryPathRepository::new(s);
    mp.migrate().await.expect("m");
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
    };
    build_router(st)
}
async fn bv(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024).await.expect("r");
    serde_json::from_slice(&b).expect("j")
}
#[tokio::test]
async fn create_then_get_project_through_http() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app().await;
    let pr = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"name":"LK","status":"active"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pb = bv(pr.into_body()).await;
    let pid = pb["project_id"].as_str().unwrap().to_owned();
    let gr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    let gb = bv(gr.into_body()).await;
    assert_eq!(gb["project_id"], pid);
}
#[tokio::test]
async fn paused_status_round_trip() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app().await;
    let pr = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"name":"P","status":"paused"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pb = bv(pr.into_body()).await;
    assert_eq!(pb["status"], "paused");
    let pid = pb["project_id"].as_str().unwrap();
    let gr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    assert_eq!(bv(gr.into_body()).await["status"], "paused");
}
#[tokio::test]
async fn project_survives_fresh_connection() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let d = db();
    let mut c1 = StoreConfig::from_env();
    c1.namespace = "lighting_test".to_owned();
    c1.database = d.clone();
    let s1 = SurrealStore::connect(&c1).await.expect("c");
    let mp1 = SurrealMemoryPathRepository::new(s1);
    mp1.migrate().await.expect("m");
    let a1 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp1))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
    });
    let pr = a1
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name":"P2"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pid = bv(pr.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut c2 = StoreConfig::from_env();
    c2.namespace = "lighting_test".to_owned();
    c2.database = d;
    let s2 = SurrealStore::connect(&c2).await.expect("c");
    let mp2 = SurrealMemoryPathRepository::new(s2);
    mp2.migrate().await.expect("m");
    let a2 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp2))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
    });
    let gr = a2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/projects/{pid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    assert_eq!(bv(gr.into_body()).await["project_id"], pid);
}
