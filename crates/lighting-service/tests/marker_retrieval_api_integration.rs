//! Live SurrealDB integration tests for marker-led retrieval.

use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use lighting_service::episode_ops::EpisodeService;
use lighting_service::marker_ops::MarkerService;
use lighting_service::marker_retrieval_ops::MarkerRetrievalService;
use lighting_service::source_ops::SourceService;
use lighting_service::{AppState, EpisodeAssociationService, ProjectService, build_router};
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
fn db_name() -> String {
    format!("lret_{}", Uuid::new_v4().simple())
}

async fn full_app() -> Router {
    dotenvy::dotenv().ok();
    let db = db_name();
    let mut c = StoreConfig::from_env();
    c.storage = "remote-surreal".to_owned();
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
        retrieval_service: Some(MarkerRetrievalService::new(
            Arc::clone(&mr),
            Arc::clone(&sr),
        )),
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
    })
}
async fn body_json(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("readable");
    serde_json::from_slice(&b).expect("json")
}

// A. Full marker-led path
#[tokio::test]
async fn full_marker_led_retrieval_path() {
    if skip() {
        return;
    }
    let app = full_app().await;

    // Create Source
    let content = "episode one content here!!\nepisode two content here!!\n";
    let source_r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"S","kind":"plain_text","content":content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let source_id = body_json(source_r.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Create two Episodes
    let ep1_r = app.clone().oneshot(Request::builder().method(http::Method::POST).uri("/api/v1/episodes").header(http::header::CONTENT_TYPE, "application/json").body(Body::from(json!({"title":"Episode One","source_id":source_id,"start_byte":0,"end_byte":26}).to_string())).unwrap()).await.unwrap();
    assert_eq!(ep1_r.status(), StatusCode::CREATED);
    let ep1_id = body_json(ep1_r.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let ep2_r = app.clone().oneshot(Request::builder().method(http::Method::POST).uri("/api/v1/episodes").header(http::header::CONTENT_TYPE, "application/json").body(Body::from(json!({"title":"Episode Two","source_id":source_id,"start_byte":27,"end_byte":53}).to_string())).unwrap()).await.unwrap();
    assert_eq!(ep2_r.status(), StatusCode::CREATED);
    let ep2_id = body_json(ep2_r.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Create Marker
    let mk_r = app
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
    let marker_id = body_json(mk_r.into_body()).await["marker_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Link Marker to both Episodes
    app.clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{ep1_id}/markers"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"marker_id":marker_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    app.clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{ep2_id}/markers"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"marker_id":marker_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Retrieve with different casing/whitespace
    let ret_r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"  HUMAN   Network Cable  "}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ret_r.status(), StatusCode::OK);
    let body = body_json(ret_r.into_body()).await;

    assert_eq!(body["query"], "  HUMAN   Network Cable  ");
    let m = &body["marker"];
    assert_eq!(m["display_text"], "human network cable");
    assert_eq!(m["lookup_key"], "human network cable");

    let eps = body["episodes"].as_array().unwrap();
    assert_eq!(eps.len(), 2, "should return both linked episodes");

    // Excerpts should be non-empty and contain expected text (order-independent)
    let excerpts: Vec<&str> = eps.iter().map(|e| e["excerpt"].as_str().unwrap()).collect();
    assert!(excerpts.iter().any(|x| x.contains("episode one")));
    assert!(excerpts.iter().any(|x| x.contains("episode two")));
    // why_matched explanations present
    for ep in eps.iter() {
        assert!(
            ep["why_matched"]
                .as_str()
                .unwrap()
                .contains("human network cable")
        );
    }
}

// B. Unknown phrase
#[tokio::test]
async fn unknown_phrase_returns_no_marker_with_warning() {
    if skip() {
        return;
    }
    let app = full_app().await;
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":"completely unknown phrase"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_json(r.into_body()).await;
    assert!(body["marker"].is_null());
    assert!(body["episodes"].as_array().unwrap().is_empty());
    assert!(
        body["warnings"].as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .contains("No exact Marker")
    );
}

// C. Unlinked Marker
#[tokio::test]
async fn unlinked_marker_returns_marker_with_warning() {
    if skip() {
        return;
    }
    let app = full_app().await;

    let mk_r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"text":"lonely marker"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let marker_id = body_json(mk_r.into_body()).await["marker_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/markers")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"text":"lonely marker"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = body_json(r.into_body()).await;
    assert!(!body["marker"].is_null());
    assert_eq!(body["marker"]["marker_id"], marker_id);
    assert!(body["episodes"].as_array().unwrap().is_empty());
    assert!(
        body["warnings"].as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .contains("not linked")
    );
}
