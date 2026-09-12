use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use lighting_service::episode_ops::EpisodeService;
use lighting_service::source_ops::SourceService;
use lighting_service::{AppState, build_router};
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
use serde_json::{Value, json};
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
    format!("le_{}", Uuid::new_v4().simple())
}
async fn app() -> Router {
    dotenvy::dotenv().ok();
    let d = db();
    let mut c = StoreConfig::from_env();
    c.storage = "remote-surreal".to_owned();
    c.namespace = "lighting_test".to_owned();
    c.database = d;
    let s = SurrealStore::connect(&c).await.expect("c");
    let src = SurrealSourceRepository::new(s.clone());
    src.migrate().await.expect("m");
    let mp = SurrealMemoryPathRepository::new(s);
    mp.migrate().await.expect("m");
    let sr: Arc<dyn lighting_core::SourceRepository> = Arc::new(src);
    let mr: Arc<dyn lighting_core::MemoryPathRepository> = Arc::new(mp);
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&sr))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&sr), Arc::clone(&mr))),
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
    };
    build_router(st)
}
async fn bv(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024).await.expect("r");
    serde_json::from_slice(&b).expect("j")
}
async fn cs(a: &Router, content: &str) -> String {
    let r = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"T","kind":"plain_text","content":content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    bv(r.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn create_then_get_exact_episode_excerpt() {
    if skip() {
        return;
    }
    let a = app().await;
    let content = "Line zero\nLine one\nLine two\nLine three\nLine four\n";
    let sid = cs(&a, content).await;
    let sb: usize = 15;
    let eb: usize = 28;
    let expected = &content[sb..eb];
    let pr = a.clone().oneshot(Request::builder().method(http::Method::POST).uri("/api/v1/episodes").header(http::header::CONTENT_TYPE, "application/json").body(Body::from(json!({"title":"The Human Relay Problem","source_id":sid,"start_byte":sb,"end_byte":eb}).to_string())).unwrap()).await.unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pb = bv(pr.into_body()).await;
    assert_eq!(pb["excerpt"], expected);
    let eid = pb["episode_id"].as_str().unwrap().to_owned();
    let gr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    assert_eq!(bv(gr.into_body()).await["excerpt"], expected);
}
#[tokio::test]
async fn utf8_boundary_rejection_returns_400() {
    if skip() {
        return;
    }
    let a = app().await;
    let sid = cs(&a, "fooébar").await;
    let pr = a
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/episodes")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Bad","source_id":sid,"start_byte":3,"end_byte":4}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::BAD_REQUEST);
    assert_eq!(bv(pr.into_body()).await["code"], "invalid_episode_range");
}
#[tokio::test]
async fn episode_survives_fresh_connection_with_exact_excerpt() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let d = db();
    let mut c1 = StoreConfig::from_env();
    c1.storage = "remote-surreal".to_owned();
    c1.namespace = "lighting_test".to_owned();
    c1.database = d.clone();
    let s1 = SurrealStore::connect(&c1).await.expect("c");
    let src1 = SurrealSourceRepository::new(s1.clone());
    src1.migrate().await.expect("m");
    let mp1 = SurrealMemoryPathRepository::new(s1);
    mp1.migrate().await.expect("m");
    let ar1: Arc<dyn lighting_core::SourceRepository> = Arc::new(src1);
    let a1 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&ar1))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&ar1), Arc::new(mp1))),
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
    });
    let content = "Persistence test content line\nSecond line here\n";
    let sid = cs(&a1, content).await;
    let sb: usize = 0;
    let eb: usize = 31;
    let expected = &content[sb..eb];
    let pr = a1
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/episodes")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"P","source_id":sid,"start_byte":sb,"end_byte":eb}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let eid = bv(pr.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut c2 = StoreConfig::from_env();
    c2.storage = "remote-surreal".to_owned();
    c2.namespace = "lighting_test".to_owned();
    c2.database = d;
    let s2 = SurrealStore::connect(&c2).await.expect("c");
    let src2 = SurrealSourceRepository::new(s2.clone());
    src2.migrate().await.expect("m");
    let mp2 = SurrealMemoryPathRepository::new(s2);
    mp2.migrate().await.expect("m");
    let ar2: Arc<dyn lighting_core::SourceRepository> = Arc::new(src2);
    let a2 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(SourceService::new(Arc::clone(&ar2))),
        project_service: None,
        marker_service: None,
        episode_service: Some(EpisodeService::new(Arc::clone(&ar2), Arc::new(mp2))),
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
    });
    let gr = a2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/episodes/{eid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    assert_eq!(bv(gr.into_body()).await["excerpt"], expected);
}
