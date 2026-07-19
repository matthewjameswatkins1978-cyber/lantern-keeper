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
        Source, SourceId, SourceKind, SourceRepository, SourceRepositoryError, SourceTitle,
        StoreSourceResult,
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

        async fn get_current(
            &self,
            _kind: SourceKind,
            _title: &SourceTitle,
        ) -> Result<Option<Source>, SourceRepositoryError> {
            // Stub: return the last stored source (simplified for router tests).
            Ok(self.last_stored.lock().unwrap().clone())
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

// ── Marker retrieval revision-safe excerpt tests (LK-024) ─────────────────

#[cfg(test)]
mod marker_revision_tests {
    use std::sync::{Arc, Mutex};

    use lighting_core::{
        find_all_matches, Episode, EpisodeId, EpisodeMarkerLink, EpisodeProjectLink, EpisodeTitle,
        Marker, MarkerId, MemoryPathRepository, MemoryPathRepositoryError, NewSource, Project,
        ProjectId, Source, SourceContent, SourceId, SourceKind, SourceRange, SourceRepository,
        SourceRepositoryError, SourceTitle, StoreMarkerResult, StoreSourceResult,
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
