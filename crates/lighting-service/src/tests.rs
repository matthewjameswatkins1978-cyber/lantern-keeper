//! Application/router tests that do not require a live database.
//!
//! These tests verify request mapping, validation, and error shapes.
//! They use a stub `SourceRepository` to simulate different outcomes.

#[cfg(test)]
mod router_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{self, Request, StatusCode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use lighting_core::{
        Source, SourceId, SourceKind, SourceRepository, SourceRepositoryError, SourceTitle,
        StoreSourceResult,
    };

    use crate::source_ops::SourceService;
    use crate::{AppState, build_router};

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

        async fn get_current(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            // Stub: return the last stored source (simplified for router tests).
            Ok(self.last_stored.lock().unwrap().clone())
        }

        async fn list_all_by_kind_and_title(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Vec<Source>, SourceRepositoryError> {
            Ok(self.last_stored.lock().unwrap().iter().cloned().collect())
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
            project_retrieval_service: None,
            tethers_client: None,
            memory_service: None,
            ledger_service: None,
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
    // LK-027: File-capture tests (first capture, unchanged re-capture,
    // changed re-capture creates revision)
    // ---------------------------------------------------------------------------

    /// A stub that supports kind+title logical grouping for revision tests.
    struct LogicalRepo {
        sources: std::sync::Mutex<Vec<Source>>,
    }

    impl LogicalRepo {
        fn new() -> Self {
            Self {
                sources: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl SourceRepository for LogicalRepo {
        async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
            let mut sources = self.sources.lock().unwrap();
            // Look for existing source with same kind + title
            // Find the latest existing source with same kind+title
            let existing_idx = sources
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| {
                    s.kind() == source.kind() && s.title().as_str() == source.title().as_str()
                })
                .map(|(i, _)| i);
            if let Some(idx) = existing_idx {
                let existing = &sources[idx];
                if existing.fingerprint() == source.fingerprint() {
                    // Unchanged — duplicate
                    Ok(StoreSourceResult::Duplicate {
                        existing_id: existing.id().clone(),
                        attempted: source,
                    })
                } else {
                    // Changed — create revision linked to previous
                    let previous_id = existing.id().clone();
                    sources.push(source.clone());
                    // We need to return Stored with previous_version_id set.
                    // But StoreSourceResult::Stored wraps Source, and the source
                    // already has previous_version_id=None from create().
                    // We simulate by storing and returning the source as-is.
                    // In real SurrealDB, the store sets previous_version_id.
                    // For test, we construct a source with previous_version_id.
                    let revised = Source::reconstitute(
                        source.id().clone(),
                        source.kind(),
                        source.title().clone(),
                        source.content().clone(),
                        source.fingerprint().clone(),
                        source.created_at(),
                        Some(previous_id),
                    );
                    sources.push(revised.clone());
                    Ok(StoreSourceResult::Stored(revised))
                }
            } else {
                // Fresh store
                sources.push(source.clone());
                Ok(StoreSourceResult::Stored(source))
            }
        }

        async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
            Ok(self
                .sources
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id() == id)
                .cloned())
        }

        async fn get_current(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            Ok(self.sources.lock().unwrap().last().cloned())
        }

        async fn list_all_by_kind_and_title(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Vec<Source>, SourceRepositoryError> {
            Ok(self.sources.lock().unwrap().clone())
        }
    }

    fn logical_app(repo: Arc<dyn SourceRepository>) -> axum::Router {
        let service = SourceService::new(repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: Some(service),
            project_service: None,
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
            project_retrieval_service: None,
            tethers_client: None,
            memory_service: None,
            ledger_service: None,
        };
        build_router(state)
    }

    #[tokio::test]
    async fn first_file_capture_returns_stored_without_previous_id() {
        let repo = Arc::new(LogicalRepo::new());
        let app = logical_app(repo);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/notes.md",
                            "kind": "markdown",
                            "content": "# First capture\n\nHello world"
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
        // First capture must NOT have previous_source_id
        assert!(body.get("previous_source_id").is_none() || body["previous_source_id"].is_null());
    }

    #[tokio::test]
    async fn unchanged_recapture_returns_duplicate_with_same_id() {
        let repo = Arc::new(LogicalRepo::new());
        let app = logical_app(repo);

        let payload = json!({
            "title": "/d/projects/doc.md",
            "kind": "plain_text",
            "content": "unchanging content"
        })
        .to_string();

        // First capture
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

        // Second capture — same content, same title
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
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
    async fn changed_recapture_creates_revision_with_previous_source_id() {
        let repo = Arc::new(LogicalRepo::new());
        let app = logical_app(repo);

        // First capture
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/evolving.md",
                            "kind": "markdown",
                            "content": "# Version 1\n\nInitial draft"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let b1: Value = body_as_json(r1.into_body()).await;
        let first_id = b1["source_id"].as_str().unwrap().to_owned();

        // Second capture — same title, different content
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/evolving.md",
                            "kind": "markdown",
                            "content": "# Version 2\n\nRevised with new ideas\nMore text"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(r2.status(), StatusCode::CREATED);
        let b2: Value = body_as_json(r2.into_body()).await;
        assert_eq!(b2["outcome"], "stored");
        let second_id = b2["source_id"].as_str().unwrap().to_owned();
        // New revision should have a different ID
        assert_ne!(second_id, first_id);
        // Must carry the previous source ID
        assert_eq!(
            b2["previous_source_id"].as_str().unwrap(),
            first_id,
            "revision must link to previous source"
        );
    }

    // ---------------------------------------------------------------------------
    // LK-028: Source history tests (revision listing via API)
    // ---------------------------------------------------------------------------

    /// A stub that preserves inserted sources for history queries.
    struct HistoryRepo {
        sources: std::sync::Mutex<Vec<Source>>,
    }

    impl HistoryRepo {
        fn new() -> Self {
            Self {
                sources: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl SourceRepository for HistoryRepo {
        async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
            let mut sources = self.sources.lock().unwrap();
            // Find the latest existing source with same kind+title
            let existing_idx = sources
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| {
                    s.kind() == source.kind() && s.title().as_str() == source.title().as_str()
                })
                .map(|(i, _)| i);
            if let Some(idx) = existing_idx {
                let existing = &sources[idx];
                if existing.fingerprint() == source.fingerprint() {
                    Ok(StoreSourceResult::Duplicate {
                        existing_id: existing.id().clone(),
                        attempted: source,
                    })
                } else {
                    let previous_id = existing.id().clone();
                    let revised = Source::reconstitute(
                        source.id().clone(),
                        source.kind(),
                        source.title().clone(),
                        source.content().clone(),
                        source.fingerprint().clone(),
                        source.created_at(),
                        Some(previous_id),
                    );
                    sources.push(revised.clone());
                    Ok(StoreSourceResult::Stored(revised))
                }
            } else {
                sources.push(source.clone());
                Ok(StoreSourceResult::Stored(source))
            }
        }

        async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
            Ok(self
                .sources
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id() == id)
                .cloned())
        }

        async fn get_current(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            Ok(self.sources.lock().unwrap().last().cloned())
        }

        async fn list_all_by_kind_and_title(
            &self,
            kind: SourceKind,
            title: &SourceTitle,
        ) -> Result<Vec<Source>, SourceRepositoryError> {
            Ok(self
                .sources
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.kind() == kind && s.title().as_str() == title.as_str())
                .cloned()
                .collect())
        }
    }

    fn history_app(repo: Arc<dyn SourceRepository>) -> axum::Router {
        let service = SourceService::new(repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: Some(service),
            project_service: None,
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
            project_retrieval_service: None,
            tethers_client: None,
            memory_service: None,
            ledger_service: None,
        };
        build_router(state)
    }

    #[tokio::test]
    async fn first_capture_history_returns_one_current_revision() {
        let repo = Arc::new(HistoryRepo::new());
        let app = history_app(repo.clone());

        // Store one source
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/sources")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projs/single.md",
                            "kind": "markdown",
                            "content": "# One\nsingle revision"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);

        // Query history
        let hr = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/history?kind=markdown&title=/d/projs/single.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(hr.status(), StatusCode::OK);
        let body: Value = body_as_json(hr.into_body()).await;
        assert_eq!(body["title"], "/d/projs/single.md");
        let revs = body["revisions"].as_array().unwrap();
        assert_eq!(revs.len(), 1, "one revision expected");
        assert_eq!(revs[0]["current"], true);
        assert!(revs[0]["previous_source_id"].is_null());
    }

    #[tokio::test]
    async fn three_revision_chain_returned_oldest_to_current() {
        let repo = Arc::new(HistoryRepo::new());
        let app = history_app(repo);

        // Create 3 revisions by POSTing with same title, different content
        let titles_and_content = [
            ("/d/projs/chain.md", "# V1\nfirst"),
            ("/d/projs/chain.md", "# V2\nsecond"),
            ("/d/projs/chain.md", "# V3\nthird"),
        ];

        let mut ids = Vec::new();
        for (title, content) in &titles_and_content {
            let r = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/api/v1/sources")
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({
                                "title": title,
                                "kind": "markdown",
                                "content": content
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::CREATED);
            let b: Value = body_as_json(r.into_body()).await;
            ids.push(b["source_id"].as_str().unwrap().to_owned());
        }

        // Query history
        let hr = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/history?kind=markdown&title=/d/projs/chain.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(hr.status(), StatusCode::OK);
        let body: Value = body_as_json(hr.into_body()).await;
        let revs = body["revisions"].as_array().unwrap();
        assert_eq!(revs.len(), 3, "three revisions expected");

        // Oldest first
        assert_eq!(revs[0]["source_id"], ids[0]);
        assert_eq!(revs[0]["current"], false);
        assert!(revs[0]["previous_source_id"].is_null());

        // Middle
        assert_eq!(revs[1]["source_id"], ids[1]);
        assert_eq!(revs[1]["current"], false);
        assert_eq!(revs[1]["previous_source_id"], ids[0]);

        // Newest = current
        assert_eq!(revs[2]["source_id"], ids[2]);
        assert_eq!(revs[2]["current"], true);
        assert_eq!(revs[2]["previous_source_id"], ids[1]);
    }

    #[tokio::test]
    async fn current_marker_identifies_final_revision() {
        let repo = Arc::new(HistoryRepo::new());
        let app = history_app(repo);

        // Two revisions: A -> B
        let mut ids = Vec::new();
        for content in &["# A\nfirst pass", "# B\nsecond pass"] {
            let r = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .uri("/api/v1/sources")
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({
                                "title": "/d/projs/final.md",
                                "kind": "markdown",
                                "content": content
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::CREATED);
            let b: Value = body_as_json(r.into_body()).await;
            ids.push(b["source_id"].as_str().unwrap().to_owned());
        }
        assert_eq!(ids.len(), 2);

        let hr = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/history?kind=markdown&title=/d/projs/final.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body: Value = body_as_json(hr.into_body()).await;
        let revs = body["revisions"].as_array().unwrap();
        assert_eq!(revs.len(), 2);

        // A (oldest): not current, no previous
        assert_eq!(revs[0]["source_id"], ids[0]);
        assert_eq!(revs[0]["current"], false, "A must not be current");
        assert!(revs[0]["previous_source_id"].is_null());

        // B (newest = current): references A
        assert_eq!(revs[1]["source_id"], ids[1]);
        assert_eq!(revs[1]["current"], true, "B must be current");
        assert_eq!(revs[1]["previous_source_id"], ids[0]);
    }

    #[tokio::test]
    async fn unknown_path_history_returns_404() {
        let repo = Arc::new(HistoryRepo::new());
        let app = history_app(repo);

        let r = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/api/v1/sources/history?kind=plain_text&title=/nonexistent/file.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let body: Value = body_as_json(r.into_body()).await;
        assert_eq!(body["code"], "source_not_found");
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

// ── Marker retrieval revision-safe excerpt tests (LK-024) ─────────────────

#[cfg(test)]
mod marker_revision_tests {
    use std::sync::{Arc, Mutex};

    use lighting_core::{
        Episode, EpisodeId, EpisodeMarkerLink, EpisodeProjectLink, EpisodeTitle, Marker, MarkerId,
        MemoryPathRepository, MemoryPathRepositoryError, NewSource, Project, ProjectId, Source,
        SourceContent, SourceId, SourceKind, SourceRange, SourceRepository, SourceRepositoryError,
        SourceTitle, StoreMarkerResult, StoreSourceResult, find_all_matches,
    };

    use crate::marker_retrieval_ops::MarkerRetrievalService;

    // ── Newtype wrapper (avoids orphan rules) ───────────────────────────────

    struct Repo(Mutex<Inner>);

    struct Inner {
        sources: std::collections::HashMap<String, Source>,
        markers: std::collections::HashMap<String, Marker>,
        episodes: std::collections::HashMap<String, Episode>,
        marker_links: std::collections::HashMap<String, Vec<String>>,
    }

    impl Inner {
        fn new() -> Self {
            Self {
                sources: std::collections::HashMap::new(),
                markers: std::collections::HashMap::new(),
                episodes: std::collections::HashMap::new(),
                marker_links: std::collections::HashMap::new(),
            }
        }
    }

    impl Repo {
        fn new() -> Self {
            Self(Mutex::new(Inner::new()))
        }

        fn add_source(&self, source: Source) {
            self.0
                .lock()
                .unwrap()
                .sources
                .insert(source.id().as_str().to_owned(), source);
        }

        fn add_marker(&self, marker: Marker) {
            self.0
                .lock()
                .unwrap()
                .markers
                .insert(marker.lookup_key().to_owned(), marker);
        }

        fn add_episode(&self, episode: Episode) {
            self.0
                .lock()
                .unwrap()
                .episodes
                .insert(episode.id().as_str().to_owned(), episode);
        }

        fn link_marker_episode(&self, marker_id: &str, episode_id: &str) {
            self.0
                .lock()
                .unwrap()
                .marker_links
                .entry(marker_id.to_owned())
                .or_default()
                .push(episode_id.to_owned());
        }
    }

    #[async_trait::async_trait]
    impl SourceRepository for Repo {
        async fn store(&self, _source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
            unimplemented!("test does not store")
        }

        async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
            Ok(self.0.lock().unwrap().sources.get(id.as_str()).cloned())
        }

        async fn get_current(
            &self,
            kind: SourceKind,
            title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            let inner = self.0.lock().unwrap();
            let mut best: Option<&Source> = None;
            for source in inner.sources.values() {
                if source.kind() == kind && source.title().as_str() == title.as_str() {
                    match best {
                        None => best = Some(source),
                        Some(current) => {
                            if source.created_at() > current.created_at() {
                                best = Some(source);
                            }
                        }
                    }
                }
            }
            Ok(best.cloned())
        }

        async fn list_all_by_kind_and_title(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Vec<Source>, SourceRepositoryError> {
            Ok(self.0.lock().unwrap().sources.values().cloned().collect())
        }
    }

    #[async_trait::async_trait]
    impl MemoryPathRepository for Repo {
        async fn create_project(
            &self,
            _project: Project,
        ) -> Result<Project, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn get_project(
            &self,
            _id: &ProjectId,
        ) -> Result<Option<Project>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn create_episode(
            &self,
            _episode: Episode,
        ) -> Result<Episode, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn get_episode(
            &self,
            id: &EpisodeId,
        ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
            Ok(self.0.lock().unwrap().episodes.get(id.as_str()).cloned())
        }

        async fn create_marker(
            &self,
            _marker: Marker,
        ) -> Result<StoreMarkerResult, MemoryPathRepositoryError> {
            unimplemented!("test does not create marker")
        }

        async fn get_marker(
            &self,
            _id: &MarkerId,
        ) -> Result<Option<Marker>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn find_marker_by_lookup(
            &self,
            lookup_key: &str,
        ) -> Result<Option<Marker>, MemoryPathRepositoryError> {
            Ok(self.0.lock().unwrap().markers.get(lookup_key).cloned())
        }

        async fn link_episode_project(
            &self,
            _link: EpisodeProjectLink,
        ) -> Result<(), MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn link_episode_marker(
            &self,
            _link: EpisodeMarkerLink,
        ) -> Result<(), MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn list_episode_project_links(
            &self,
            _episode_id: &EpisodeId,
        ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn list_episode_marker_links(
            &self,
            _episode_id: &EpisodeId,
        ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn list_marker_episode_links(
            &self,
            marker_id: &MarkerId,
        ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError> {
            let links = self
                .0
                .lock()
                .unwrap()
                .marker_links
                .get(marker_id.as_str())
                .cloned()
                .unwrap_or_default();
            Ok(links
                .into_iter()
                .map(|eid| EpisodeMarkerLink::new(EpisodeId::new(eid).unwrap(), marker_id.clone()))
                .collect())
        }

        async fn list_project_episode_links(
            &self,
            _project_id: &ProjectId,
        ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn find_project_episode_by_source_range(
            &self,
            _project_id: &ProjectId,
            _source_id: &SourceId,
            _start_byte: usize,
            _end_byte: usize,
        ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn list_all_projects(&self) -> Result<Vec<Project>, MemoryPathRepositoryError> {
            unimplemented!()
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn make_source(title: &str, content: &str) -> Source {
        Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new(title).unwrap(),
            content: SourceContent::new(content).unwrap(),
        })
    }

    fn make_episode(title: &str, source_range: SourceRange) -> Episode {
        Episode::new(EpisodeTitle::new(title).unwrap(), source_range)
    }

    // ── tests ──────────────────────────────────────────────────────────────

    /// When the historical excerpt appears exactly once in the current revision,
    /// retrieval should rebase to the current source ID and its byte range.
    #[tokio::test]
    async fn uniquely_rebased_marker_retrieval_uses_current_content_and_source_id() {
        let v1_content = "Hello old world here is more text";
        let v2_content = "Hello new world here is more text OLD_DATA old world extra";

        let v1 = make_source("doc", v1_content);
        let v2 = make_source("doc", v2_content);
        let v1_content_obj = SourceContent::new(v1_content).unwrap();

        let source_range = SourceRange::new(v1.id().clone(), 6, 15, &v1_content_obj).unwrap();
        let episode = make_episode("Test Episode", source_range);
        let episode_id = episode.id().as_str().to_owned();
        let marker = Marker::new("my phrase").unwrap();

        let repo = Arc::new(Repo::new());
        repo.add_source(v1.clone());
        repo.add_source(v2.clone());
        repo.add_marker(marker.clone());
        repo.add_episode(episode);
        repo.link_marker_episode(marker.id().as_str(), &episode_id);

        let memory_repo: Arc<dyn MemoryPathRepository> = repo.clone();
        let source_repo: Arc<dyn SourceRepository> = repo.clone();

        let service = MarkerRetrievalService::new(memory_repo, source_repo);
        let result = service
            .retrieve("my phrase")
            .await
            .expect("retrieval should succeed");

        let episodes = result.episodes;
        assert_eq!(episodes.len(), 1, "one episode expected");

        let ep = &episodes[0];
        assert_eq!(
            ep.content_source_id,
            v2.id().as_str(),
            "content_source_id should be v2"
        );
        assert_eq!(
            ep.latest_source_id, None,
            "no fallback latest_source_id needed"
        );
        assert_eq!(ep.excerpt, "old world");

        let expected_pos = find_all_matches(v2_content, "old world");
        assert_eq!(expected_pos.len(), 1, "old world should appear once in v2");
        assert_eq!(ep.start_byte, expected_pos[0]);
        assert_eq!(ep.end_byte, expected_pos[0] + "old world".len());
    }

    /// When the historical excerpt appears multiple times in the current
    /// revision, retrieval keeps the historical content and flags the latest
    /// Source revision.
    #[tokio::test]
    async fn un_rebasable_marker_retrieval_keeps_historical_and_flags_latest_revision() {
        let v1_content = "the quick brown fox";
        let v2_content = "quick the quick brown fox quick";

        let v1 = make_source("notes", v1_content);
        let v2 = make_source("notes", v2_content);
        let v1_content_obj = SourceContent::new(v1_content).unwrap();

        let source_range = SourceRange::new(v1.id().clone(), 4, 9, &v1_content_obj).unwrap();
        let episode = make_episode("Ambiguous Episode", source_range);
        let episode_id = episode.id().as_str().to_owned();
        let marker = Marker::new("remember fox").unwrap();

        let repo = Arc::new(Repo::new());
        repo.add_source(v1.clone());
        repo.add_source(v2.clone());
        repo.add_marker(marker.clone());
        repo.add_episode(episode);
        repo.link_marker_episode(marker.id().as_str(), &episode_id);

        let memory_repo: Arc<dyn MemoryPathRepository> = repo.clone();
        let source_repo: Arc<dyn SourceRepository> = repo.clone();

        let service = MarkerRetrievalService::new(memory_repo, source_repo);
        let result = service
            .retrieve("remember fox")
            .await
            .expect("retrieval should succeed");

        let episodes = result.episodes;
        assert_eq!(episodes.len(), 1);

        let ep = &episodes[0];
        assert_eq!(
            ep.content_source_id,
            v1.id().as_str(),
            "content_source_id should be v1 (historical)"
        );
        assert_eq!(
            ep.latest_source_id.as_deref(),
            Some(v2.id().as_str()),
            "latest_source_id should flag v2"
        );
        assert_eq!(ep.excerpt, "quick");
        assert_eq!(ep.start_byte, 4);
        assert_eq!(ep.end_byte, 9);
    }
}

// ── LK-033: Project add-file tests ───────────────────────────────────────

#[cfg(test)]
mod project_add_file_tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{self, Request, StatusCode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use lighting_core::{
        Episode, EpisodeId, EpisodeProjectLink, MemoryPathRepository, MemoryPathRepositoryError,
        Project, ProjectId, Source, SourceId, SourceKind, SourceRepository, SourceRepositoryError,
        SourceTitle, StoreSourceResult,
    };

    use crate::project_ops::ProjectService;
    use crate::source_ops::SourceService;
    use crate::{AppState, build_router};

    // ── Stub that supports Source + Project + Episode + linking ──────────

    struct FullStub {
        inner: Mutex<FullStubInner>,
    }

    #[derive(Clone)]
    struct FullStubInner {
        sources: Vec<Source>,
        projects: std::collections::HashMap<String, Project>,
        episodes: Vec<Episode>,
        project_links: Vec<EpisodeProjectLink>,
    }

    impl FullStub {
        fn new() -> Self {
            Self {
                inner: Mutex::new(FullStubInner {
                    sources: Vec::new(),
                    projects: std::collections::HashMap::new(),
                    episodes: Vec::new(),
                    project_links: Vec::new(),
                }),
            }
        }
    }

    #[async_trait::async_trait]
    impl SourceRepository for FullStub {
        async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
            let mut inner = self.inner.lock().unwrap();
            let existing_idx = inner
                .sources
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| {
                    s.kind() == source.kind() && s.title().as_str() == source.title().as_str()
                })
                .map(|(i, _)| i);
            if let Some(idx) = existing_idx {
                let existing = &inner.sources[idx];
                if existing.fingerprint() == source.fingerprint() {
                    Ok(StoreSourceResult::Duplicate {
                        existing_id: existing.id().clone(),
                        attempted: source,
                    })
                } else {
                    let previous_id = existing.id().clone();
                    let revised = Source::reconstitute(
                        source.id().clone(),
                        source.kind(),
                        source.title().clone(),
                        source.content().clone(),
                        source.fingerprint().clone(),
                        source.created_at(),
                        Some(previous_id),
                    );
                    inner.sources.push(revised.clone());
                    Ok(StoreSourceResult::Stored(revised))
                }
            } else {
                inner.sources.push(source.clone());
                Ok(StoreSourceResult::Stored(source))
            }
        }

        async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.sources.iter().find(|s| s.id() == id).cloned())
        }

        async fn get_current(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.sources.last().cloned())
        }

        async fn list_all_by_kind_and_title(
            &self,
            kind: SourceKind,
            title: &SourceTitle,
        ) -> Result<Vec<Source>, SourceRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner
                .sources
                .iter()
                .filter(|s| s.kind() == kind && s.title().as_str() == title.as_str())
                .cloned()
                .collect())
        }
    }

    #[async_trait::async_trait]
    impl MemoryPathRepository for FullStub {
        async fn create_project(
            &self,
            project: Project,
        ) -> Result<Project, MemoryPathRepositoryError> {
            let mut inner = self.inner.lock().unwrap();
            inner
                .projects
                .insert(project.id().as_str().to_owned(), project.clone());
            Ok(project)
        }

        async fn get_project(
            &self,
            id: &ProjectId,
        ) -> Result<Option<Project>, MemoryPathRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.projects.get(id.as_str()).cloned())
        }

        async fn create_episode(
            &self,
            episode: Episode,
        ) -> Result<Episode, MemoryPathRepositoryError> {
            let mut inner = self.inner.lock().unwrap();
            inner.episodes.push(episode.clone());
            Ok(episode)
        }

        async fn get_episode(
            &self,
            id: &EpisodeId,
        ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.episodes.iter().find(|e| *e.id() == *id).cloned())
        }

        async fn create_marker(
            &self,
            _marker: lighting_core::Marker,
        ) -> Result<lighting_core::StoreMarkerResult, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn get_marker(
            &self,
            _id: &lighting_core::MarkerId,
        ) -> Result<Option<lighting_core::Marker>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn find_marker_by_lookup(
            &self,
            _lookup_key: &str,
        ) -> Result<Option<lighting_core::Marker>, MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn link_episode_project(
            &self,
            link: EpisodeProjectLink,
        ) -> Result<(), MemoryPathRepositoryError> {
            let mut inner = self.inner.lock().unwrap();
            // Only add if not already present
            let is_duplicate = inner.project_links.iter().any(|existing| {
                existing.episode_id() == link.episode_id()
                    && existing.project_id() == link.project_id()
            });
            if !is_duplicate {
                inner.project_links.push(link);
            }
            Ok(())
        }

        async fn link_episode_marker(
            &self,
            _link: lighting_core::EpisodeMarkerLink,
        ) -> Result<(), MemoryPathRepositoryError> {
            unimplemented!()
        }

        async fn list_episode_project_links(
            &self,
            _episode_id: &EpisodeId,
        ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
            Ok(self.inner.lock().unwrap().project_links.clone())
        }

        async fn list_episode_marker_links(
            &self,
            _episode_id: &EpisodeId,
        ) -> Result<Vec<lighting_core::EpisodeMarkerLink>, MemoryPathRepositoryError> {
            Ok(vec![])
        }

        async fn list_marker_episode_links(
            &self,
            _marker_id: &lighting_core::MarkerId,
        ) -> Result<Vec<lighting_core::EpisodeMarkerLink>, MemoryPathRepositoryError> {
            Ok(vec![])
        }

        async fn list_project_episode_links(
            &self,
            _project_id: &ProjectId,
        ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
            Ok(self.inner.lock().unwrap().project_links.clone())
        }

        async fn find_project_episode_by_source_range(
            &self,
            project_id: &ProjectId,
            source_id: &SourceId,
            _start_byte: usize,
            _end_byte: usize,
        ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
            let inner = self.inner.lock().unwrap();
            // Find an episode linked to this project with this source_id
            for link in &inner.project_links {
                if link.project_id() == project_id
                    && let Some(ep) = inner
                        .episodes
                        .iter()
                        .find(|e| *e.id() == *link.episode_id())
                    && ep.source_range().source_id() == source_id
                {
                    return Ok(Some(ep.clone()));
                }
            }
            Ok(None)
        }

        async fn list_all_projects(&self) -> Result<Vec<Project>, MemoryPathRepositoryError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.projects.values().cloned().collect())
        }
    }

    fn full_app(stub: Arc<FullStub>) -> axum::Router {
        let source_repo: Arc<dyn SourceRepository> = stub.clone();
        let memory_repo: Arc<dyn MemoryPathRepository> = stub;
        let source_service = SourceService::new(source_repo.clone());
        let project_service = ProjectService::new(memory_repo, source_repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        };
        build_router(state)
    }

    async fn body_as_json<T: for<'de> serde::Deserialize<'de>>(body: Body) -> T {
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .expect("body should be readable");
        serde_json::from_slice(&bytes).expect("body should be valid JSON")
    }

    async fn create_test_project(stub: &FullStub, name: &str) -> String {
        let project = Project::new(
            lighting_core::ProjectName::new(name).unwrap(),
            lighting_core::ProjectStatus::Active,
        );
        let pid = project.id().as_str().to_owned();
        stub.inner
            .lock()
            .unwrap()
            .projects
            .insert(pid.clone(), project);
        pid
    }

    // ── tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn first_add_file_produces_201_creates_source_episode_and_link() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "Demo").await;
        let app = full_app(stub.clone());

        let r = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/notes.md",
                            "kind": "markdown",
                            "content": "# Hello\n\nWorld file content"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            r.status(),
            StatusCode::CREATED,
            "first add-file should return 201"
        );
        let body: Value = body_as_json(r.into_body()).await;
        assert_eq!(body["outcome"], "stored");
        assert!(!body["source_id"].as_str().unwrap().is_empty());
        assert!(
            body["previous_source_id"].is_null() || body["previous_source_id"].as_str().is_none()
        );
        assert!(!body["episode_id"].as_str().unwrap().is_empty());
        assert_eq!(body["link_status"], "linked");

        // Verify internal state: one source, one episode, one link
        let inner = stub.inner.lock().unwrap();
        assert_eq!(inner.sources.len(), 1, "should have 1 source");
        assert_eq!(inner.episodes.len(), 1, "should have 1 episode");
        assert_eq!(inner.project_links.len(), 1, "should have 1 project link");
    }

    #[tokio::test]
    async fn unchanged_repeat_is_idempotent_no_duplicate_link() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "Demo").await;
        let app = full_app(stub.clone());

        let payload = json!({
            "title": "/d/projects/static.md",
            "kind": "plain_text",
            "content": "static content unchanged"
        })
        .to_string();

        // First call
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let b1: Value = body_as_json(r1.into_body()).await;
        let first_sid = b1["source_id"].as_str().unwrap().to_owned();
        let first_eid = b1["episode_id"].as_str().unwrap().to_owned();

        // Second call — same content, same title
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            r2.status(),
            StatusCode::OK,
            "unchanged repeat should return 200"
        );
        let b2: Value = body_as_json(r2.into_body()).await;
        assert_eq!(
            b2["outcome"], "duplicate",
            "Source capture should be duplicate"
        );
        assert_eq!(
            b2["source_id"].as_str().unwrap(),
            first_sid,
            "same source ID"
        );
        assert_eq!(b2["link_status"], "already_linked");
        assert_eq!(
            b2["episode_id"].as_str().unwrap(),
            first_eid,
            "same episode ID"
        );

        // Verify no duplicate sources, episodes, or links created
        let inner = stub.inner.lock().unwrap();
        assert_eq!(inner.sources.len(), 1, "still 1 source");
        assert_eq!(inner.episodes.len(), 1, "still 1 episode");
        assert_eq!(inner.project_links.len(), 1, "still 1 project link");
    }

    #[tokio::test]
    async fn changed_repeat_creates_revision_without_duplicate_project_link() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "Evolving").await;
        let app = full_app(stub.clone());

        // First capture
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/evolving.md",
                            "kind": "markdown",
                            "content": "# V1\ninitial"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);
        let b1: Value = body_as_json(r1.into_body()).await;
        let first_sid = b1["source_id"].as_str().unwrap().to_owned();
        let first_eid = b1["episode_id"].as_str().unwrap().to_owned();

        // Second capture — different content, same title
        let r2 = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/evolving.md",
                            "kind": "markdown",
                            "content": "# V2\nrevised with more text"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            r2.status(),
            StatusCode::OK,
            "changed file signals existing project link"
        );
        let b2: Value = body_as_json(r2.into_body()).await;
        assert_eq!(
            b2["outcome"], "revision_captured_existing_project_link",
            "outcome must signal revision was captured but no new link created"
        );
        let second_sid = b2["source_id"].as_str().unwrap().to_owned();
        assert_ne!(second_sid, first_sid, "new source ID for revision");
        assert_eq!(
            b2["previous_source_id"].as_str().unwrap(),
            first_sid,
            "links to previous source"
        );
        assert_eq!(b2["link_status"], "already_linked");
        assert_eq!(
            b2["episode_id"].as_str().unwrap(),
            first_eid,
            "same episode ID — no new Episode created"
        );

        // Verify: 2 sources (v1 + v2), but still only 1 episode and 1 link
        let inner = stub.inner.lock().unwrap();
        assert_eq!(inner.sources.len(), 2, "2 sources (v1 + v2)");
        assert_eq!(inner.episodes.len(), 1, "still 1 episode — no duplicate");
        assert_eq!(
            inner.project_links.len(),
            1,
            "still 1 project link — no duplicate"
        );
    }

    // ── LK-034: Handoff entry count test ─────────────────────────────────

    #[tokio::test]
    async fn handoff_contains_one_entry_after_revision() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "HandoffProof").await;
        let app = full_app(stub.clone());

        // First add-file
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/handoff.md",
                            "kind": "markdown",
                            "content": "# V1\nfirst version of handoff file"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);

        // Change and re-add — should NOT create duplicate links
        let r2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/add-file"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "title": "/d/projects/handoff.md",
                            "kind": "markdown",
                            "content": "# V2\nsecond version of handoff file with more text"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::OK);

        // Verify internal counts
        let inner = stub.inner.lock().unwrap();
        let link_count = inner.project_links.len();
        let episode_count = inner.episodes.len();
        assert_eq!(link_count, 1, "handoff must have exactly 1 project link");
        assert_eq!(episode_count, 1, "handoff must have exactly 1 episode");
    }

    // ── Tethers preview handler tests ───────────────────────────────

    fn tethers_full_app(
        stub: Arc<FullStub>,
        tethers_client: crate::tethers_engine_client::TethersEngineClient,
    ) -> axum::Router {
        let source_repo: Arc<dyn SourceRepository> = stub.clone();
        let memory_repo: Arc<dyn MemoryPathRepository> = stub;
        let source_service = SourceService::new(source_repo.clone());
        let project_service = ProjectService::new(memory_repo, source_repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: Some(source_service),
            project_service: Some(project_service),
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
            project_retrieval_service: None,
            tethers_client: Some(tethers_client),
            memory_service: None,
            ledger_service: None,
        };
        build_router(state)
    }

    #[tokio::test]
    async fn matched_response_crosses_http_boundary() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "MatchedProject").await;

        let matched = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("eval-m-1".into()),
            event_id: Some("evt-m-1".into()),
            tether_id: Some("t-m-1".into()),
            tether_version: Some("v-m-1".into()),
            status: crate::tethers_preview::TethersStatus::Matched,
            plan: Some(crate::tethers_preview::Plan {
                id: "eval-m-1/plan".into(),
                required_effects: vec!["lantern.write".into()],
                actions: vec![crate::tethers_preview::PlannedAction {
                    action_id: "a1".into(),
                    idempotency_key: "eval-m-1/a1".into(),
                    capability: "lantern.task.record".into(),
                    capability_version: "1.0.0".into(),
                    arguments: serde_json::json!({"project": "lantern-keeper"}),
                    effects: vec!["lantern.write".into()],
                }],
            }),
            trail: Some(vec![
                crate::tethers_preview::TrailEntry {
                    sequence: 1,
                    phase: "reception".into(),
                    kind: "event_received".into(),
                    outcome: "accepted".into(),
                    message: "Received event".into(),
                },
                crate::tethers_preview::TrailEntry {
                    sequence: 2,
                    phase: "evaluation".into(),
                    kind: "condition_checked".into(),
                    outcome: "matched".into(),
                    message: "Condition passed".into(),
                },
            ]),
            error: None,
        };

        let client = crate::tethers_engine_client::TethersEngineClient::fixed(matched);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-39",
                            "changed_files": 3,
                            "evaluation_id": "eval-m-1",
                            "event_id": "evt-m-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = body_as_json(response.into_body()).await;

        assert_eq!(body["status"], "matched");
        // Plan contains capability "lantern.task.record"
        let plan = &body["plan"];
        let actions = plan["actions"].as_array().expect("actions array");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["capability"], "lantern.task.record");

        // Trail sequence remains ordered
        let trail = body["trail"].as_array().expect("trail array");
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[0]["sequence"], 1);
        assert_eq!(trail[1]["sequence"], 2);
        assert!(trail[0]["sequence"].as_u64().unwrap() < trail[1]["sequence"].as_u64().unwrap());
    }

    #[tokio::test]
    async fn not_matched_response_crosses_http_boundary() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "NotMatchedProject").await;

        let not_matched = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("eval-nm-1".into()),
            event_id: Some("evt-nm-1".into()),
            tether_id: Some("t-nm-1".into()),
            tether_version: Some("v-nm-1".into()),
            status: crate::tethers_preview::TethersStatus::NotMatched,
            plan: None,
            trail: Some(vec![crate::tethers_preview::TrailEntry {
                sequence: 1,
                phase: "evaluation".into(),
                kind: "condition_checked".into(),
                outcome: "not_matched".into(),
                message: "condition false".into(),
            }]),
            error: None,
        };

        let client = crate::tethers_engine_client::TethersEngineClient::fixed(not_matched);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-40",
                            "changed_files": 0,
                            "evaluation_id": "eval-nm-1",
                            "event_id": "evt-nm-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = body_as_json(response.into_body()).await;

        assert_eq!(body["status"], "not_matched");

        // The plan key exists and is JSON null
        let plan_val = body
            .get("plan")
            .expect("plan key must exist in not_matched response");
        assert!(
            plan_val.is_null(),
            "plan must be JSON null, got: {plan_val}"
        );

        // Trail is present
        let trail = body["trail"].as_array().expect("trail array present");
        assert_eq!(trail.len(), 1);
    }

    #[tokio::test]
    async fn minimal_error_remains_minimal_over_http() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "MinErrProject").await;

        let minimal_err = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: None,
            event_id: None,
            tether_id: None,
            tether_version: None,
            status: crate::tethers_preview::TethersStatus::Error,
            plan: None,
            trail: None,
            error: Some(crate::tethers_preview::TethersError {
                code: "parse_error".into(),
                message: "unexpected token".into(),
            }),
        };

        let client = crate::tethers_engine_client::TethersEngineClient::fixed(minimal_err);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-41",
                            "changed_files": 1,
                            "evaluation_id": "eval-me-1",
                            "event_id": "evt-me-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = body_as_json(response.into_body()).await;

        // Exact semantic JSON
        assert_eq!(body["protocol_version"], "0.1");
        assert_eq!(body["status"], "error");
        assert_eq!(body["error"]["code"], "parse_error");
        assert_eq!(body["error"]["message"], "unexpected token");

        // Explicitly assert these keys are absent
        assert!(
            body.get("evaluation_id").is_none(),
            "evaluation_id must be absent"
        );
        assert!(body.get("event_id").is_none(), "event_id must be absent");
        assert!(body.get("tether_id").is_none(), "tether_id must be absent");
        assert!(
            body.get("tether_version").is_none(),
            "tether_version must be absent"
        );
        assert!(body.get("plan").is_none(), "plan must be absent");
        assert!(body.get("trail").is_none(), "trail must be absent");
    }

    #[tokio::test]
    async fn correlated_error_crosses_http_boundary() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "CorrErrProject").await;

        let correlated_err = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("eval-ce-1".into()),
            event_id: Some("evt-ce-1".into()),
            tether_id: Some("t-ce-1".into()),
            tether_version: Some("v-ce-1".into()),
            status: crate::tethers_preview::TethersStatus::Error,
            plan: None,
            trail: Some(vec![
                crate::tethers_preview::TrailEntry {
                    sequence: 1,
                    phase: "reception".into(),
                    kind: "event_received".into(),
                    outcome: "accepted".into(),
                    message: "Received event".into(),
                },
                crate::tethers_preview::TrailEntry {
                    sequence: 2,
                    phase: "evaluation".into(),
                    kind: "condition_failed".into(),
                    outcome: "error".into(),
                    message: "Fact missing".into(),
                },
            ]),
            error: Some(crate::tethers_preview::TethersError {
                code: "missing_fact".into(),
                message: "Fact not found".into(),
            }),
        };

        let client = crate::tethers_engine_client::TethersEngineClient::fixed(correlated_err);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-42",
                            "changed_files": 2,
                            "evaluation_id": "eval-ce-1",
                            "event_id": "evt-ce-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = body_as_json(response.into_body()).await;

        // Status is "error"
        assert_eq!(body["status"], "error");

        // All four correlation identifiers remain present
        assert_eq!(body["evaluation_id"], "eval-ce-1");
        assert_eq!(body["event_id"], "evt-ce-1");
        assert_eq!(body["tether_id"], "t-ce-1");
        assert_eq!(body["tether_version"], "v-ce-1");

        // Plan key exists and is JSON null
        let plan_val = body
            .get("plan")
            .expect("plan key must exist in correlated error response");
        assert!(
            plan_val.is_null(),
            "plan must be JSON null, got: {plan_val}"
        );

        // Error code/message remain present
        assert_eq!(body["error"]["code"], "missing_fact");
        assert_eq!(body["error"]["message"], "Fact not found");

        // Trail remains present and ordered
        let trail = body["trail"].as_array().expect("trail array present");
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[0]["sequence"], 1);
        assert_eq!(trail[1]["sequence"], 2);
        assert!(trail[0]["sequence"].as_u64().unwrap() < trail[1]["sequence"].as_u64().unwrap());
    }

    #[tokio::test]
    async fn preview_route_is_registered_for_post() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "RouteRegProject").await;

        let matched = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("eval-rr-1".into()),
            event_id: Some("evt-rr-1".into()),
            tether_id: Some("t-rr-1".into()),
            tether_version: Some("v-rr-1".into()),
            status: crate::tethers_preview::TethersStatus::Matched,
            plan: None,
            trail: None,
            error: None,
        };

        let client = crate::tethers_engine_client::TethersEngineClient::fixed(matched);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-50",
                            "changed_files": 1,
                            "evaluation_id": "eval-rr-1",
                            "event_id": "evt-rr-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["status"], "matched");
    }

    #[tokio::test]
    async fn preview_route_rejects_wrong_method() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "WrongMethodProject").await;
        let app = full_app(stub);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn missing_engine_configuration_is_503() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "MissingEngineProject").await;

        let source_repo: Arc<dyn SourceRepository> = stub.clone();
        let memory_repo: Arc<dyn MemoryPathRepository> = stub;
        let source_service = SourceService::new(source_repo.clone());
        let project_service = ProjectService::new(memory_repo, source_repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        };
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-51",
                            "changed_files": 1,
                            "evaluation_id": "eval-me-2",
                            "event_id": "evt-me-2"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "tethers_unavailable");
    }

    #[tokio::test]
    async fn engine_failure_is_sanitised_502() {
        let stub = Arc::new(FullStub::new());
        let project_id = create_test_project(&stub, "EngineFailProject").await;

        let engine_err = crate::tethers_engine_client::TethersEngineError::NonZeroExit {
            code: 37,
            stderr: "raw stderr output with /absolute/path/to/engine\nand OS error details".into(),
            stderr_truncated: false,
        };
        let client = crate::tethers_engine_client::TethersEngineClient::fixed_error(engine_err);
        let app = tethers_full_app(stub, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{project_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-52",
                            "changed_files": 1,
                            "evaluation_id": "eval-ef-1",
                            "event_id": "evt-ef-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "tethers_engine_error");

        let message = body["message"].as_str().unwrap();
        // Must not contain absolute engine path
        assert!(
            !message.contains("/absolute/path/to/engine"),
            "response must not leak engine path, got: {message}"
        );
        // Must not contain raw stderr
        assert!(
            !message.contains("raw stderr"),
            "response must not leak raw stderr, got: {message}"
        );
        // Must not contain OS error details
        assert!(
            !message.contains("OS error"),
            "response must not leak OS error details, got: {message}"
        );
    }

    #[tokio::test]
    async fn unknown_project_preserves_404() {
        let stub = Arc::new(FullStub::new());
        // Do not create a Project — a valid UUID will still 404.

        let matched = crate::tethers_preview::TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("eval-up-1".into()),
            event_id: Some("evt-up-1".into()),
            tether_id: None,
            tether_version: None,
            status: crate::tethers_preview::TethersStatus::Matched,
            plan: None,
            trail: None,
            error: None,
        };
        let client = crate::tethers_engine_client::TethersEngineClient::fixed(matched);

        let source_repo: Arc<dyn SourceRepository> = stub.clone();
        let memory_repo: Arc<dyn MemoryPathRepository> = stub;
        let source_service = SourceService::new(source_repo.clone());
        let project_service = ProjectService::new(memory_repo, source_repo);
        let state = AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: Some(source_service),
            project_service: Some(project_service),
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
            project_retrieval_service: None,
            tethers_client: Some(client),
            memory_service: None,
            ledger_service: None,
        };
        let app = build_router(state);

        // A valid UUID that does not exist in the stub
        let unknown_id = "a0a0a0a0-a0a0-a0a0-a0a0-a0a0a0a0a0a0";

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/api/v1/projects/{unknown_id}/tethers/preview"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-53",
                            "changed_files": 1,
                            "evaluation_id": "eval-up-1",
                            "event_id": "evt-up-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "project_not_found");
    }

    #[tokio::test]
    async fn invalid_project_id_preserves_400() {
        let stub = Arc::new(FullStub::new());
        let app = full_app(stub);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/v1/projects/%20%20/tethers/preview")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "task": "LK-54",
                            "changed_files": 1,
                            "evaluation_id": "eval-ip-1",
                            "event_id": "evt-ip-1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = body_as_json(response.into_body()).await;
        assert_eq!(body["code"], "invalid_project_id");
    }
}
