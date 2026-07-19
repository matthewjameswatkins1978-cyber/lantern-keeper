//! Application-layer Source operations.
//!
//! Coordinates between the domain (`lighting-core`) and the SurrealDB
//! repository (`lighting-store-surreal`) without duplicating storage logic.
//! The HTTP layer depends on this module, not directly on the repository.

use std::sync::Arc;

use lighting_core::{
    NewSource, SourceContent, SourceId, SourceKind, SourceRepository, SourceTitle,
    StoreSourceResult,
};

use crate::source_dto::{
    ApiError, CreateSourceResponse, SourceHistoryItem, SourceHistoryResponse, SourceResponse,
};

/// Application-level error when Source operations fail.
#[derive(Debug, thiserror::Error)]
pub enum SourceOperationError {
    #[error("source repository is not available")]
    Unavailable,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<SourceOperationError> for ApiError {
    fn from(error: SourceOperationError) -> Self {
        match error {
            SourceOperationError::Unavailable => ApiError::storage_unavailable(),
            SourceOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

/// Wrapper around the trait object, providing application-level coordination.
#[derive(Clone)]
pub struct SourceService {
    repo: Arc<dyn SourceRepository>,
}

impl SourceService {
    /// Creates a new `SourceService` backed by the given repository.
    pub fn new(repo: Arc<dyn SourceRepository>) -> Self {
        Self { repo }
    }

    /// Accepts source creation input, constructs the domain Source,
    /// and delegates to the repository. Returns the API-level outcome.
    pub async fn create_source(
        &self,
        title: String,
        kind: SourceKind,
        content: String,
    ) -> Result<CreateSourceResponse, SourceOperationError> {
        let title = SourceTitle::new(title).map_err(|_| {
            // This should never happen because we validate before calling.
            // But if it does, treat it as an internal error, not bad
            // validation feedback with content leakage.
            SourceOperationError::Repository(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid title",
            )))
        })?;

        let content = SourceContent::new(content).map_err(|_| {
            SourceOperationError::Repository(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid content",
            )))
        })?;

        let source = lighting_core::Source::create(NewSource {
            kind,
            title,
            content,
        });

        let source_id = source.id().clone();

        let result = self
            .repo
            .store(source)
            .await
            .map_err(|e| SourceOperationError::Repository(e.into()))?;

        match result {
            StoreSourceResult::Stored(stored_source) => {
                let previous_id = stored_source.previous_version_id().cloned();
                Ok(CreateSourceResponse::stored(
                    &source_id,
                    previous_id.as_ref(),
                ))
            }
            StoreSourceResult::Duplicate { existing_id, .. } => {
                Ok(CreateSourceResponse::duplicate(&existing_id))
            }
        }
    }

    /// Lists all revisions of a logical Source, oldest first, marking the current one.
    pub async fn list_history(
        &self,
        kind: SourceKind,
        title: &str,
    ) -> Result<Option<SourceHistoryResponse>, SourceOperationError> {
        let title_obj = SourceTitle::new(title).map_err(|_| {
            SourceOperationError::Repository(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid title for history lookup",
            )))
        })?;

        let sources = self
            .repo
            .list_all_by_kind_and_title(kind, &title_obj)
            .await
            .map_err(|e| SourceOperationError::Repository(e.into()))?;

        if sources.is_empty() {
            return Ok(None);
        }

        // Determine current: the source whose ID is not referenced by any
        // other source's previous_version_id.
        let referenced: std::collections::HashSet<&SourceId> = sources
            .iter()
            .filter_map(|s| s.previous_version_id())
            .collect();

        let kind_str = match kind {
            SourceKind::PlainText => "plain_text",
            SourceKind::Markdown => "markdown",
        };
        let items: Vec<SourceHistoryItem> = sources
            .iter()
            .map(|s| {
                let is_current = !referenced.contains(s.id());
                SourceHistoryItem {
                    source_id: s.id().as_str().to_owned(),
                    previous_source_id: s.previous_version_id().map(|p| p.as_str().to_owned()),
                    created_at: s.created_at(),
                    current: is_current,
                }
            })
            .collect();

        Ok(Some(SourceHistoryResponse {
            title: title.to_owned(),
            kind: kind_str.to_owned(),
            revisions: items,
        }))
    }

    /// Retrieves a Source by ID and returns the API DTO.
    pub async fn get_source(
        &self,
        id: &SourceId,
    ) -> Result<Option<SourceResponse>, SourceOperationError> {
        let source = self
            .repo
            .get(id)
            .await
            .map_err(|e| SourceOperationError::Repository(e.into()))?;

        Ok(source.as_ref().map(SourceResponse::from_domain))
    }
}

/// Creates a no-op `SourceService` for tests that shouldn't touch a real DB.
///
/// Every operation returns `Unavailable` to simulate the repository not being
/// wired.
#[cfg(test)]
pub fn unavailable_service() -> SourceService {
    struct UnavailableRepo;
    #[async_trait::async_trait]
    impl SourceRepository for UnavailableRepo {
        async fn store(
            &self,
            _source: lighting_core::Source,
        ) -> Result<StoreSourceResult, lighting_core::SourceRepositoryError> {
            Err(lighting_core::SourceRepositoryError::Operation(Box::new(
                std::io::Error::new(std::io::ErrorKind::NotConnected, "unavailable"),
            )))
        }
        async fn get(
            &self,
            _id: &SourceId,
        ) -> Result<Option<lighting_core::Source>, lighting_core::SourceRepositoryError> {
            Err(lighting_core::SourceRepositoryError::Operation(Box::new(
                std::io::Error::new(std::io::ErrorKind::NotConnected, "unavailable"),
            )))
        }

        async fn get_current(
            &self,
            _kind: lighting_core::SourceKind,
            _title: &lighting_core::SourceTitle,
        ) -> Result<Option<lighting_core::Source>, lighting_core::SourceRepositoryError> {
            Err(lighting_core::SourceRepositoryError::Operation(Box::new(
                std::io::Error::new(std::io::ErrorKind::NotConnected, "unavailable"),
            )))
        }

        async fn list_all_by_kind_and_title(
            &self,
            _kind: lighting_core::SourceKind,
            _title: &lighting_core::SourceTitle,
        ) -> Result<Vec<lighting_core::Source>, lighting_core::SourceRepositoryError> {
            Err(lighting_core::SourceRepositoryError::Operation(Box::new(
                std::io::Error::new(std::io::ErrorKind::NotConnected, "unavailable"),
            )))
        }
    }

    SourceService::new(Arc::new(UnavailableRepo))
}
