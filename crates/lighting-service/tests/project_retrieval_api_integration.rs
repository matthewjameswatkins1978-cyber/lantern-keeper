//! Live SurrealDB integration tests for project-scoped retrieval.

use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use lighting_service::episode_ops::EpisodeService;
use lighting_service::marker_ops::MarkerService;
use lighting_service::marker_retrieval_ops::MarkerRetrievalService;
use lighting_service::project_retrieval_ops::ProjectRetrievalService;
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
fn db() -> String {
    format!("lpr_{}", Uuid::new_v4().simple())
}

async fn app(d: &str) -> Router {
    dotenvy::dotenv().ok();
    let mut c = StoreConfig::from_env();
    c.storage = "remote-surreal".to_owned();
    c.namespace = "lighting_test".to_owned();
    c.database = d.to_string();
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
        project_retrieval_service: Some(ProjectRetrievalService::new(
            Arc::clone(&mr),
            Arc::clone(&sr),
        )),
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
    })
}
async fn app_default() -> Router {
    app(&db()).await
}

async fn bj(body: Body) -> Value {
    let b = axum::body::to_bytes(body, 1024 * 1024).await.expect("r");
    serde_json::from_slice(&b).expect("j")
}
async fn cs(app: &Router, content: &str) -> String {
    let r = app
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
    bj(r.into_body()).await["source_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn ce(app: &Router, sid: &str, tb: &str, s: usize, e: usize) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/episodes")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":tb,"source_id":sid,"start_byte":s,"end_byte":e}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    bj(r.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn cp(app: &Router, name: &str) -> String {
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
    bj(r.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn record_result(app: &Router, pid: &str, title: &str, content: &str) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/record-result"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":title,"kind":"markdown","content":content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    (status, bj(r.into_body()).await)
}

#[tokio::test]
async fn project_retrieval_returns_exact_authoritative_excerpts() {
    if skip() {
        return;
    }
    let app = app_default().await;
    // Deliberate leading/trailing whitespace & Unicode to prove excerpt is unchanged.
    let content = "  episode one here  \n  episode two hëre  \n";
    let sid = cs(&app, content).await;
    let eid1 = ce(&app, &sid, "Episode One", 0, 21).await;
    let eid2 = ce(&app, &sid, "Episode Two", 22, content.len()).await;
    let pid = cp(&app, "Test Project").await;
    // link via HTTP
    app.clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid1}/projects"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":pid,"kind":"primary"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    app.clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/episodes/{eid2}/projects"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":pid,"kind":"primary"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    // retrieve
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"project_id":pid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = bj(r.into_body()).await;
    assert_eq!(body["project"]["name"], "Test Project");
    assert_eq!(body["project"]["status"], "active");
    let eps = body["episodes"].as_array().unwrap();
    assert_eq!(eps.len(), 2);

    // --- context_package heading and audience ---
    assert_eq!(body["context_package"]["format"], "markdown");
    assert_eq!(body["context_package"]["audience"], "codex");
    let handoff = body["context_package"]["content"].as_str().unwrap();
    assert!(handoff.contains("# Codex Handoff Context"));

    // --- Project name and Project ID appear in the package ---
    assert!(handoff.contains("Test Project"));
    assert!(handoff.contains(pid.as_str()));

    // --- ordering: first Episode in JSON appears before second in package ---
    let eid1_str = eps[0]["episode_id"].as_str().unwrap();
    let eid2_str = eps[1]["episode_id"].as_str().unwrap();
    let pos1 = handoff.find(eid1_str).unwrap();
    let pos2 = handoff.find(eid2_str).unwrap();
    assert!(
        pos1 < pos2,
        "first Episode must appear before second in context_package content"
    );

    // --- every Episode field appears in the package ---
    for ep in eps {
        let ep_id = ep["episode_id"].as_str().unwrap();
        let src_id = ep["source_id"].as_str().unwrap();
        let start = ep["start_byte"].as_u64().unwrap();
        let end = ep["end_byte"].as_u64().unwrap();
        let excerpt = ep["excerpt"].as_str().unwrap();
        let why = ep["why_matched"].as_str().unwrap();

        assert!(
            handoff.contains(ep_id),
            "handoff must contain episode ID {ep_id}"
        );
        assert!(
            handoff.contains(src_id),
            "handoff must contain source ID {src_id}"
        );
        let byte_range = format!("{}..{}", start, end);
        assert!(
            handoff.contains(&byte_range),
            "handoff must contain byte range {byte_range}"
        );
        assert!(
            handoff.contains(why),
            "handoff must contain why_matched text"
        );
        assert!(
            handoff.contains(excerpt),
            "handoff must contain exact excerpt unchanged"
        );
    }

    // --- deliberate whitespace/Unicode preserved in at least one excerpt ---
    let excerpts: Vec<&str> = eps.iter().map(|e| e["excerpt"].as_str().unwrap()).collect();
    let unicode_excerpt = excerpts
        .iter()
        .find(|x| x.contains("hëre"))
        .expect("at least one excerpt must contain Unicode 'hëre'");
    assert!(
        unicode_excerpt.starts_with(" episode two hëre"),
        "excerpt must preserve leading whitespace exactly: got {unicode_excerpt:?}"
    );
    assert!(
        unicode_excerpt.ends_with("hëre  \n"),
        "excerpt must preserve trailing whitespace exactly: got {unicode_excerpt:?}"
    );

    // --- why_matched uses project name ---
    assert!(
        eps.iter()
            .all(|e| e["why_matched"].as_str().unwrap().contains("Test Project"))
    );
}

#[tokio::test]
async fn record_result_is_idempotent_and_appears_in_project_handoff() {
    if skip() {
        return;
    }
    let app = app_default().await;
    let pid = cp(&app, "Writeback Project").await;
    let content =
        "# Result\r\n\r\nImplemented handoff writeback.\r\nPreserve café whitespace.  \r\n";
    let content_len = content.len();

    let (first_status, first) = record_result(&app, &pid, "Codex Result", content).await;
    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(first["outcome"], "recorded");
    assert_eq!(first["project_id"], pid);
    assert_eq!(first["start_byte"], 0);
    assert_eq!(first["end_byte"], content_len);
    let source_id = first["source_id"].as_str().unwrap().to_owned();
    let episode_id = first["episode_id"].as_str().unwrap().to_owned();

    let (second_status, second) = record_result(&app, &pid, "Different Rerun Title", content).await;
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(second["outcome"], "already_recorded");
    assert_eq!(second["source_id"], source_id);
    assert_eq!(second["episode_id"], episode_id);

    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"project_id":pid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = bj(r.into_body()).await;
    let episodes = body["episodes"].as_array().unwrap();
    assert_eq!(
        episodes.len(),
        1,
        "rerun must not duplicate Project-linked Episodes"
    );
    let ep = &episodes[0];
    assert_eq!(ep["episode_id"], episode_id);
    assert_eq!(ep["source_id"], source_id);
    assert_eq!(ep["start_byte"], 0);
    assert_eq!(ep["end_byte"], content_len);
    assert_eq!(ep["excerpt"], content);
    assert!(
        ep["why_matched"]
            .as_str()
            .unwrap()
            .contains("Writeback Project")
    );

    let handoff = body["context_package"]["content"].as_str().unwrap();
    assert!(handoff.contains(&episode_id));
    assert!(handoff.contains(&source_id));
    assert!(handoff.contains(&format!("bytes 0..{content_len}")));
    assert!(handoff.contains(content));
    assert!(handoff.contains("Episode is linked to Project \"Writeback Project\"."));
}

#[tokio::test]
async fn record_result_missing_project_returns_safe_404() {
    if skip() {
        return;
    }
    let app = app_default().await;
    let secret = "secret-result-content-LK020";
    let (status, body) = record_result(
        &app,
        "00000000-0000-0000-0000-000000000000",
        "Missing Project Result",
        secret,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "project_not_found");
    let body_string = body.to_string();
    assert!(!body_string.contains(secret));
    assert!(!body_string.to_lowercase().contains("surreal"));
}

#[tokio::test]
async fn unknown_project_returns_404() {
    if skip() {
        return;
    }
    let app = app_default().await;
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"project_id":"00000000-0000-0000-0000-000000000000"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(bj(r.into_body()).await["code"], "project_not_found");
}

#[tokio::test]
async fn unlinked_project_returns_honest_empty_result() {
    if skip() {
        return;
    }
    let app = app_default().await;
    let pid = cp(&app, "Lonely Project").await;
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"project_id":pid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = bj(r.into_body()).await;
    assert_eq!(body["project"]["name"], "Lonely Project");
    assert!(body["episodes"].as_array().unwrap().is_empty());
    assert_eq!(
        body["warnings"].as_array().unwrap()[0],
        "Project exists but is not linked to any Episodes."
    );
    // context_package must exist and be honest
    assert!(
        body.get("context_package").is_some(),
        "must include context_package"
    );
    let handoff = body["context_package"]["content"].as_str().unwrap();
    assert!(
        handoff.contains("No Episodes are currently linked to this Project."),
        "handoff must state exactly: No Episodes are currently linked to this Project. \
         Got: {handoff}"
    );
    assert!(
        !handoff.contains("```text"),
        "handoff must not contain any fabricated excerpt block"
    );
}

#[tokio::test]
async fn project_retrieval_with_missing_authoritative_source_returns_safe_409() {
    if skip() {
        return;
    }
    let db_name = db();
    let app = app(&db_name).await;
    let secret_fragment = "secret-marker-LK-016A";
    let content = format!("episode with {} here\n", secret_fragment);
    let sid = cs(&app, &content).await;
    let eid = ce(&app, &sid, "Broken Episode", 0, content.len()).await;
    let pid = cp(&app, "Source-Broken Project").await;
    // Link Episode → Project
    app.clone()
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
    // Delete the Source from the isolated database
    let mut sc = StoreConfig::from_env();
    sc.storage = "remote-surreal".to_owned();
    sc.namespace = "lighting_test".to_owned();
    sc.database = db_name;
    let test_store = SurrealStore::connect(&sc).await.expect("connect");
    test_store
        .query(format!(
            "DELETE source WHERE id = type::record('source', \"{}\")",
            sid
        ))
        .await
        .expect("delete source");
    // Now retrieve — should fail with 409
    let r = app
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/retrieval/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"project_id":pid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::CONFLICT,
        "must return 409 for missing Source"
    );
    let body = bj(r.into_body()).await;
    assert_eq!(body["code"], "source_unavailable");
    let body_str = body.to_string();
    // Must not leak source content
    assert!(!body_str.contains(secret_fragment));
    // Must not leak SurrealDB internals
    for forbidden in &["surreal", "SELECT", "RELATE", "DELETE"] {
        assert!(
            !body_str.to_lowercase().contains(&forbidden.to_lowercase()),
            "must not leak {forbidden}"
        );
    }
    // Must not return a success body
    assert!(body.get("project").is_none());
    assert!(body.get("episodes").is_none());
    // Must not return a context_package
    assert!(
        body.get("context_package").is_none(),
        "must not return a context_package on 409"
    );
}
