use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use lighting_service::marker_ops::MarkerService;
use lighting_service::{AppState, build_router};
use lighting_store_surreal::{StoreConfig, SurrealMemoryPathRepository, SurrealStore};
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
    format!("lm_{}", Uuid::new_v4().simple())
}
async fn app() -> Router {
    dotenvy::dotenv().ok();
    let d = db();
    let mut c = StoreConfig::from_env();
    c.storage = "remote-surreal".to_owned();
    c.namespace = "lighting_test".to_owned();
    c.database = d;
    let s = SurrealStore::connect(&c).await.expect("c");
    let mp = SurrealMemoryPathRepository::new(s);
    mp.migrate().await.expect("m");
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: None,
        marker_service: Some(MarkerService::new(Arc::new(mp))),
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
        authority_service: None,
    };
    build_router(st)
}
async fn bv(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024).await.expect("r");
    serde_json::from_slice(&b).expect("j")
}
#[tokio::test]
async fn create_then_get_marker_through_http() {
    if skip() {
        return;
    }
    let a = app().await;
    let pr = a
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
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pb = bv(pr.into_body()).await;
    let mid = pb["marker_id"].as_str().unwrap().to_owned();
    let gr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/markers/{mid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    let gb = bv(gr.into_body()).await;
    assert_eq!(gb["marker_id"], mid);
}
#[tokio::test]
async fn normalised_duplicate_returns_200_with_original_id() {
    if skip() {
        return;
    }
    let a = app().await;
    let r1 = a
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
    let b1 = bv(r1.into_body()).await;
    let oid = b1["marker_id"].as_str().unwrap().to_owned();
    let r2 = a
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
    assert_eq!(bv(r2.into_body()).await["marker_id"], oid);
}
#[tokio::test]
async fn lookup_via_different_casing_whitespace_finds_original() {
    if skip() {
        return;
    }
    let a = app().await;
    let r1 = a
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
    let oid = bv(r1.into_body()).await["marker_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let lr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/markers/lookup?text=HUMAN+++network++cable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(lr.status(), StatusCode::OK);
    assert_eq!(bv(lr.into_body()).await["marker_id"], oid);
}
