use axum::Router;
use axum::body::Body;
use axum::http::{self, Request, StatusCode};
use lighting_service::project_ops::ProjectService;
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
    format!("lp_{}", Uuid::new_v4().simple())
}
/// Create an app with SourceService wired so `project-add-file` works.
async fn app_with_source() -> Router {
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
    let project_service = ProjectService::new(Arc::new(mp), Arc::clone(&sr));
    let source_service = SourceService::new(Arc::clone(&sr));
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: Some(source_service),
        project_service: Some(project_service),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
        authority_service: None,
        dreamer_service: None,
    };
    build_router(st)
}

async fn app() -> Router {
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
    let st = AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp), Arc::clone(&sr))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
        authority_service: None,
        dreamer_service: None,
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
    c1.storage = "remote-surreal".to_owned();
    c1.namespace = "lighting_test".to_owned();
    c1.database = d.clone();
    let s1 = SurrealStore::connect(&c1).await.expect("c");
    let src1 = SurrealSourceRepository::new(s1.clone());
    src1.migrate().await.expect("m");
    let sr1: Arc<dyn lighting_core::SourceRepository> = Arc::new(src1);
    let mp1 = SurrealMemoryPathRepository::new(s1);
    mp1.migrate().await.expect("m");
    let a1 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp1), Arc::clone(&sr1))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
        authority_service: None,
        dreamer_service: None,
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
    c2.storage = "remote-surreal".to_owned();
    c2.namespace = "lighting_test".to_owned();
    c2.database = d;
    let s2 = SurrealStore::connect(&c2).await.expect("c");
    let src2 = SurrealSourceRepository::new(s2.clone());
    src2.migrate().await.expect("m");
    let sr2: Arc<dyn lighting_core::SourceRepository> = Arc::new(src2);
    let mp2 = SurrealMemoryPathRepository::new(s2);
    mp2.migrate().await.expect("m");
    let a2 = build_router(AppState {
        ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        source_service: None,
        project_service: Some(ProjectService::new(Arc::new(mp2), Arc::clone(&sr2))),
        marker_service: None,
        episode_service: None,
        association_service: None,
        retrieval_service: None,
        project_retrieval_service: None,
        tethers_client: None,
        memory_service: None,
        ledger_service: None,
        epistemic_service: None,
        authority_service: None,
        dreamer_service: None,
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

#[tokio::test]
async fn list_projects_empty_returns_200_with_empty_array() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app().await;
    let r = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = bv(r.into_body()).await;
    let projects = body["projects"]
        .as_array()
        .expect("projects should be an array");
    assert!(
        projects.is_empty(),
        "empty DB should return empty projects list"
    );
}

#[tokio::test]
async fn list_projects_returns_both_in_create_order() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app().await;

    // Create two projects
    let r1 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name": "Alpha"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = bv(r1.into_body()).await;
    let pid1 = b1["project_id"].as_str().unwrap().to_owned();
    let name1 = b1["name"].as_str().unwrap().to_owned();
    let status1 = b1["status"].as_str().unwrap().to_owned();

    let r2 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name": "Zebra"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::CREATED);
    let b2 = bv(r2.into_body()).await;
    let pid2 = b2["project_id"].as_str().unwrap().to_owned();
    let name2 = b2["name"].as_str().unwrap().to_owned();

    // List — must return both in created_at ASC order
    let list_r = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_r.status(), StatusCode::OK);

    let body = bv(list_r.into_body()).await;
    let projects = body["projects"]
        .as_array()
        .expect("projects should be an array");
    assert_eq!(projects.len(), 2, "should return both projects");

    // First should be Alpha (created first)
    assert_eq!(projects[0]["project_id"].as_str().unwrap(), pid1);
    assert_eq!(projects[0]["name"].as_str().unwrap(), name1);
    assert_eq!(projects[0]["status"].as_str().unwrap(), status1);

    // Second should be Zebra (created second)
    assert_eq!(projects[1]["project_id"].as_str().unwrap(), pid2);
    assert_eq!(projects[1]["name"].as_str().unwrap(), name2);
    assert_eq!(projects[1]["status"].as_str().unwrap(), "active");
}

// ── Project-show integration tests ──────────────────────────────────────

#[tokio::test]
async fn show_empty_project_returns_metadata_and_empty_episodes() {
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
                .body(Body::from(json!({"name": "Empty"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pid = bv(pr.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();

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
    assert_eq!(gb["name"], "Empty");
    assert_eq!(gb["status"], "active");
    let episodes = gb["episodes"]
        .as_array()
        .expect("episodes must be an array");
    assert!(
        episodes.is_empty(),
        "empty project must have empty episodes"
    );
}

#[tokio::test]
async fn show_project_with_linked_file_returns_episode_details() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app_with_source().await;

    // Create project
    let pr = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name": "Linked"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pid = bv(pr.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Add a file
    let title = "/d/projects/linked.md";
    let ar = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/add-file"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "title": title,
                        "kind": "markdown",
                        "content": "# V1\ninitial"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ar.status(), StatusCode::CREATED);
    let ab = bv(ar.into_body()).await;
    let source_id = ab["source_id"].as_str().unwrap().to_owned();
    let episode_id = ab["episode_id"].as_str().unwrap().to_owned();

    // Show the project
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

    let episodes = gb["episodes"]
        .as_array()
        .expect("episodes must be an array");
    assert_eq!(episodes.len(), 1);

    let ep = &episodes[0];
    assert_eq!(ep["episode_id"], episode_id);
    assert_eq!(ep["link_kind"], "primary");
    assert_eq!(ep["source_id"], source_id);
    assert_eq!(ep["start_byte"], 0);
    assert_eq!(ep["end_byte"].as_u64().unwrap(), 12); // "# V1\ninitial".len()
    assert_eq!(ep["source_title"], title);
    assert_eq!(ep["source_kind"], "markdown");
    assert!(ep["latest_source_id"].is_null(), "no revision yet");
}

#[tokio::test]
async fn show_after_revision_preserves_historical_source_with_latest_flag() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app_with_source().await;

    // Create project
    let pr = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name": "Rev"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pid = bv(pr.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let title = "/d/projects/revised.md";
    // First capture
    let r1 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/add-file"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": title, "kind": "markdown", "content": "# V1\nfirst"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let b1 = bv(r1.into_body()).await;
    let first_source_id = b1["source_id"].as_str().unwrap().to_owned();
    let episode_id = b1["episode_id"].as_str().unwrap().to_owned();

    // Second capture (revised content)
    let r2 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/add-file"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": title, "kind": "markdown", "content": "# V2\nsecond revision"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let b2 = bv(r2.into_body()).await;
    let second_source_id = b2["source_id"].as_str().unwrap().to_owned();
    assert_ne!(second_source_id, first_source_id);
    assert_eq!(b2["previous_source_id"], first_source_id);
    assert_eq!(b2["episode_id"], episode_id);

    // Show project — must preserve historical source_id with latest_source_id
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

    let episodes = gb["episodes"]
        .as_array()
        .expect("episodes must be an array");
    assert_eq!(
        episodes.len(),
        1,
        "still one project-visible Episode after revision"
    );

    let ep = &episodes[0];
    assert_eq!(ep["episode_id"], episode_id);
    assert_eq!(
        ep["source_id"], first_source_id,
        "historical source_id must remain the first capture"
    );
    assert_eq!(
        ep["latest_source_id"], second_source_id,
        "latest_source_id must point to the newer revision"
    );
    assert_eq!(ep["link_kind"], "primary");
    assert_eq!(ep["source_title"], title);
    assert_eq!(ep["source_kind"], "markdown");
}

#[tokio::test]
async fn show_unknown_project_returns_404_with_documented_contract() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app().await;
    let gr = a
        .oneshot(
            Request::builder()
                .method(http::Method::GET)
                .uri("/api/v1/projects/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gr.status(), StatusCode::NOT_FOUND);
    let gb = bv(gr.into_body()).await;
    assert_eq!(gb["code"], "project_not_found");
    assert!(
        gb["message"]
            .as_str()
            .unwrap()
            .contains("No Project exists")
    );
}

#[tokio::test]
async fn show_two_episodes_in_deterministic_order() {
    if skip() {
        return;
    }
    dotenvy::dotenv().ok();
    let a = app_with_source().await;

    // Create project
    let pr = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri("/api/v1/projects")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"name": "Ordered"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pr.status(), StatusCode::CREATED);
    let pid = bv(pr.into_body()).await["project_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Add two files
    let r1 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/add-file"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/a/first.md", "kind": "markdown", "content": "# First"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let eid1 = bv(r1.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let r2 = a
        .clone()
        .oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("/api/v1/projects/{pid}/add-file"))
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title": "/b/second.md", "kind": "markdown", "content": "# Second"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::CREATED);
    let eid2 = bv(r2.into_body()).await["episode_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Show project — Episodes must be in deterministic order (created_at ASC)
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

    let episodes = gb["episodes"]
        .as_array()
        .expect("episodes must be an array");
    assert_eq!(episodes.len(), 2);
    assert_eq!(episodes[0]["episode_id"], eid1);
    assert_eq!(episodes[1]["episode_id"], eid2);
}
