//! Live SurrealDB integration tests for the Source HTTP API.
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use axum::Router;
use lighting_service::source_ops::SourceService;
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{StoreConfig, SurrealSourceRepository, SurrealStore};
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
fn tc() -> StoreConfig {
    dotenvy::dotenv().ok();
    let mut c = StoreConfig::from_env();
    c.namespace = "lighting_test".to_owned();
    c.database = format!("lighting_api_test_{}", Uuid::new_v4().simple());
    c
}
async fn cr() -> SurrealSourceRepository {
    let c = tc();
    let s = SurrealStore::connect(&c)
        .await
        .unwrap_or_else(|e| panic!("SurrealDB: {e}"));
    let r = SurrealSourceRepository::new(s);
    r.migrate().await.expect("migrate");
    r
}
fn ca(repo: Arc<SurrealSourceRepository>) -> Router {
    let svc = SourceService::new(repo);
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(svc),
        project_service: None,
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
    };
    build_router(st)
}
async fn bv(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024).await.expect("read");
    serde_json::from_slice(&b).expect("json")
}

#[tokio::test]
async fn store_markdown_and_retrieve_exact_content() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let repo = Arc::new(repo);
    let app = ca(repo.clone());
    let content = "# Handbook\n\n  leading\r\n\n\ntrailing  \t";
    let crr = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"API test handbook","kind":"markdown","content":content})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(crr.status(), StatusCode::CREATED);
    let cb = bv(crr.into_body()).await;
    let sid = cb["source_id"].as_str().unwrap().to_owned();
    let gr = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/sources/{sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    let gb = bv(gr.into_body()).await;
    assert_eq!(gb["source_id"], sid);
    assert_eq!(gb["content"], content);
}

#[tokio::test]
async fn duplicate_content_returns_same_id_without_new_record() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let repo = Arc::new(repo);
    let app = ca(repo);
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"First","kind":"markdown","content":"dup"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = bv(r1.into_body()).await;
    let fid = b1["source_id"].as_str().unwrap().to_owned();
    let r2 = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Second","kind":"markdown","content":"dup"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let b2 = bv(r2.into_body()).await;
    assert_eq!(b2["outcome"], "duplicate");
    assert_eq!(b2["source_id"].as_str().unwrap(), fid);
}

#[tokio::test]
async fn source_survives_fresh_connection() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let db = format!("lighting_api_reconnect_{}", Uuid::new_v4().simple());
    let mut c1 = StoreConfig::from_env();
    c1.namespace = "lighting_test".to_owned();
    c1.database = db.clone();
    let mut c2 = StoreConfig::from_env();
    c2.namespace = "lighting_test".to_owned();
    c2.database = db.clone();
    let s1 = SurrealStore::connect(&c1).await.expect("c");
    let r1 = SurrealSourceRepository::new(s1);
    r1.migrate().await.expect("m");
    let r1 = Arc::new(r1);
    let a1 = ca(r1);
    let content = "persistence check content";
    let crr = a1
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"P","kind":"plain_text","content":content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(crr.status(), StatusCode::CREATED);
    let cb = bv(crr.into_body()).await;
    let sid = cb["source_id"].as_str().unwrap().to_owned();
    let s2 = SurrealStore::connect(&c2).await.expect("c");
    let r2 = SurrealSourceRepository::new(s2);
    r2.migrate().await.expect("m");
    let r2 = Arc::new(r2);
    let a2 = ca(r2);
    let gr = a2
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri(format!("/api/v1/sources/{sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::OK);
    assert_eq!(bv(gr.into_body()).await["content"], content);
}

// ── LK-057 Source outline live tests ──────────────────────────────────────

#[tokio::test]
async fn outline_returns_200_with_current_headings() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let repo = Arc::new(repo);
    let app = ca(repo.clone());
    let content = "# Top\n\nintro\n\n## Child\n\nchild text\n\n# Sibling\n";
    let crr = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/d/outline-live.md", "kind": "markdown", "content": content})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(crr.status(), StatusCode::CREATED);
    let sid = bv(crr.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let hr = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/sources/outline?kind=markdown&title=/d/outline-live.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hr.status(), StatusCode::OK);
    let body = bv(hr.into_body()).await;
    assert_eq!(body["source_id"], sid);
    assert_eq!(body["kind"], "markdown");
    assert!(body["created_at"].is_string());

    let headings = body["headings"].as_array().unwrap();
    assert_eq!(headings.len(), 3);
    assert_eq!(headings[0]["level"], 1);
    assert_eq!(headings[0]["title"], "Top");
    assert_eq!(headings[1]["level"], 2);
    assert_eq!(headings[1]["title"], "Child");
    assert_eq!(headings[2]["level"], 1);
    assert_eq!(headings[2]["title"], "Sibling");
}

#[tokio::test]
async fn outline_after_revision_returns_latest_revision_id() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let repo = Arc::new(repo);
    let app = ca(repo.clone());

    // First capture
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/d/evolving-outline.md", "kind": "markdown", "content": "# V1\n\nold"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let v1_id = bv(r1.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Second capture — revised content
    let r2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/d/evolving-outline.md", "kind": "markdown", "content": "# V2\n\nnew"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::CREATED);
    let v2_id = bv(r2.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(v2_id, v1_id);

    // Outline must return the latest (v2) ID, not the historical v1
    let hr = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/sources/outline?kind=markdown&title=/d/evolving-outline.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hr.status(), StatusCode::OK);
    let body = bv(hr.into_body()).await;
    assert_eq!(
        body["source_id"], v2_id,
        "outline must report current revision ID"
    );
}

#[tokio::test]
async fn outline_unknown_source_returns_404() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let app = ca(Arc::new(repo));
    let hr = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/sources/outline?kind=markdown&title=/never-captured.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hr.status(), StatusCode::NOT_FOUND);
    let body = bv(hr.into_body()).await;
    assert_eq!(body["code"], "source_not_found");
}

#[tokio::test]
async fn outline_non_markdown_source_returns_422() {
    if skip() {
        return;
    }
    let repo = cr().await;
    let repo = Arc::new(repo);
    let app = ca(repo.clone());

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/sources")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/d/plain-outline.txt", "kind": "plain_text", "content": "Just text"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);

    let hr = app
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/sources/outline?kind=plain_text&title=/d/plain-outline.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hr.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = bv(hr.into_body()).await;
    assert_eq!(body["code"], "source_not_markdown");
}
