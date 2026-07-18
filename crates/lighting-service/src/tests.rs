//! Application/router tests that do not require a live database.
//!
//! These tests verify request mapping, validation, and error shapes.
//! They use a stub `SourceRepository` to simulate different outcomes.

#[cfg(test)]
mod router_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{self, Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use lighting_core::{
        Source, SourceId, SourceRepository, SourceRepositoryError, StoreSourceResult,
    };

    use crate::source_ops::SourceService;
    use crate::{build_router, AppState};

    // ---------------------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------------------

    /// A stub repository that records the last stored source.
    struct StubRepo {
        last_stored: std::sync::Mutex<Option<Source>>,
        fail_on_store: std::sync::atomic::AtomicBool,
    }

    impl StubRepo {
        fn new() -> Self {
            Self {
                last_stored: std::sync::Mutex::new(None),
                fail_on_store: std::sync::atomic::AtomicBool::new(false),
            }
        }

        fn set_fail(&self, fail: bool) {
            self.fail_on_store
                .store(fail, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl SourceRepository for StubRepo {
        async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
            if self.fail_on_store.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(SourceRepositoryError::Operation(Box::new(
                    std::io::Error::other("injected failure"),
                )));
            }
            let _id = source.id().clone();
            // Check for duplicate by fingerprint (simplified: same content = duplicate)
            let is_dup = {
                let guard = self.last_stored.lock().unwrap();
                if let Some(ref existing) = *guard {
                    existing.fingerprint() == source.fingerprint()
                } else {
                    false
                }
            };
            if is_dup {
                let existing_id = self
                    .last_stored
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|s| s.id().clone())
                    .unwrap();
                Ok(StoreSourceResult::Duplicate {
                    existing_id,
                    attempted: source,
                })
            } else {
                self.last_stored.lock().unwrap().replace(source.clone());
                Ok(StoreSourceResult::Stored(source))
            }
        }

        async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
            let guard = self.last_stored.lock().unwrap();
            match guard.as_ref() {
                Some(source) if source.id() == id => Ok(Some(source.clone())),
                _ => Ok(None),
            }
        }
    }

    fn test_app(repo: Arc<dyn SourceRepository>) -> axum::Router {
        let service = SourceService::new(repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: Some(service),
            project_service: None,
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
        };
        build_router(state)
    }

    fn test_app_no_storage() -> axum::Router {
        let state = AppState::new_unready();
        build_router(state)
    }

    async fn body_as_json<T: for<'de> serde::Deserialize<'de>>(body: Body) -> T {
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .expect("body should be readable");
        serde_json::from_slice(&bytes).expect("body should be valid JSON")
    }

    // ---------------------------------------------------------------------------
    // POST /api/v1/sources
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn create_source_returns_201_for_valid_markdown() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Test handbook",
                            "kind": "markdown",
                            "content": "# Hello\n\nWorld"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["outcome"], "stored");
        assert!(!body["source_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_source_returns_201_for_valid_plain_text() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Plain note",
                            "kind": "plain_text",
                            "content": "Just some text"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn duplicate_content_returns_200_with_existing_id() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo.clone());

        // First POST
        let payload = json!({
            "title": "First",
            "kind": "markdown",
            "content": "duplicate body"
        })
        .to_string();

        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let b1: Value = body_as_json(r1.into_body()).await;
        let first_id = b1["source_id"].as_str().unwrap().to_owned();

        // Second POST — same content, different title
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Second",
                            "kind": "markdown",
                            "content": "duplicate body"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(r2.status(), StatusCode::OK);
        let b2: Value = body_as_json(r2.into_body()).await;
        assert_eq!(b2["outcome"], "duplicate");
        assert_eq!(b2["source_id"].as_str().unwrap(), first_id);
    }

    #[tokio::test]
    async fn invalid_title_returns_400() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "   ",
                            "kind": "markdown",
                            "content": "body"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "invalid_source");
    }

    #[tokio::test]
    async fn empty_content_returns_400() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Title",
                            "kind": "markdown",
                            "content": ""
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "invalid_source");
    }

    #[tokio::test]
    async fn unsupported_kind_returns_400() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Title",
                            "kind": "unknown_format",
                            "content": "content"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "invalid_source");
    }

    /// Error responses must never echo source content back.
    #[tokio::test]
    async fn error_response_does_not_leak_source_content() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let secret = "xyzzy-secret-content-marker-LK-004";

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "",
                            "kind": "markdown",
                            "content": secret
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = body_as_json(response.into_body()).await;
        let body_str = body.to_string();
        assert!(
            !body_str.contains(secret),
            "Error response must not leak source content, but got: {body_str}"
        );
    }

    #[tokio::test]
    async fn unavailable_storage_returns_503() {
        let app = test_app_no_storage();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Title",
                            "kind": "markdown",
                            "content": "content"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "storage_unavailable");
    }

    #[tokio::test]
    async fn repository_failure_returns_500() {
        let repo = Arc::new(StubRepo::new());
        repo.set_fail(true);
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Title",
                            "kind": "markdown",
                            "content": "valid content"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "internal_error");
        // Verify no raw database error text leaks.
        let body_str = body.to_string();
        assert!(!body_str.contains("injected failure"));
    }

    // ---------------------------------------------------------------------------
    // GET /api/v1/sources/{source_id}
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn get_source_returns_200_with_full_response() {
        let repo = Arc::new(StubRepo::new());

        // First, store a source via the API
        let app = test_app(repo.clone());
        let create_r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "Retrieval test",
                            "kind": "markdown",
                            "content": "# Exact\ncontent"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_r.status(), StatusCode::CREATED);
        let create_body: Value = body_as_json(create_r.into_body()).await;
        let source_id = create_body["source_id"].as_str().unwrap();

        // Now GET it
        let get_r = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/api/v1/sources/{source_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(get_r.status(), StatusCode::OK);
        let get_body: Value = body_as_json(get_r.into_body()).await;
        assert_eq!(get_body["source_id"], source_id);
        assert_eq!(get_body["title"], "Retrieval test");
        assert_eq!(get_body["kind"], "markdown");
        assert_eq!(get_body["content"], "# Exact\ncontent");
        assert!(!get_body["fingerprint"].as_str().unwrap().is_empty());
        assert!(get_body["created_at"].is_string());
    }

    #[tokio::test]
    async fn get_missing_source_returns_404() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/00000000-0000-0000-0000-000000000000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "source_not_found");
    }

    #[tokio::test]
    async fn malformed_source_id_returns_400() {
        let repo = Arc::new(StubRepo::new());
        let app = test_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/not-a-uuid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "invalid_source_id");
    }

    #[tokio::test]
    async fn get_source_returns_503_when_storage_unavailable() {
        let app = test_app_no_storage();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/00000000-0000-0000-0000-000000000001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "storage_unavailable");
    }
}
