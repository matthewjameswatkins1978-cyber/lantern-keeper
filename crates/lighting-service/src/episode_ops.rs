//! Application-layer Episode operations.

use crate::episode_dto::EpisodeResponse;
use crate::source_dto::ApiError;
use lighting_core::{
    Episode, EpisodeId, EpisodeTitle, MemoryPathRepository, SourceId, SourceRange, SourceRepository,
};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum EpisodeOperationError {
    #[error("source or memory-path repository is not available")]
    Unavailable,
    #[error("episode title must not be blank")]
    BlankTitle,
    #[error("referenced Source does not exist")]
    SourceNotFound,
    #[error("no Episode exists with the given ID")]
    EpisodeNotFound,
    #[error("the Source referenced by this Episode cannot be loaded")]
    SourceUnavailable,
    #[error("invalid byte range for the referenced Source")]
    InvalidRange(#[source] lighting_core::SourceRangeError),
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<EpisodeOperationError> for ApiError {
    fn from(error: EpisodeOperationError) -> Self {
        match error {
            EpisodeOperationError::Unavailable => ApiError::storage_unavailable(),
            EpisodeOperationError::BlankTitle => ApiError {
                code: "invalid_episode".to_owned(),
                message: "Episode title must not be blank".to_owned(),
            },
            EpisodeOperationError::SourceNotFound => ApiError {
                code: "source_not_found".to_owned(),
                message: "No Source exists with the given ID".to_owned(),
            },
            EpisodeOperationError::SourceUnavailable => ApiError {
                code: "source_unavailable".to_owned(),
                message: "The Source referenced by this Episode cannot be loaded".to_owned(),
            },
            EpisodeOperationError::InvalidRange(inner) => ApiError {
                code: "invalid_episode_range".to_owned(),
                message: inner.to_string(),
            },
            EpisodeOperationError::EpisodeNotFound => ApiError {
                code: "episode_not_found".to_owned(),
                message: "No Episode exists with the given ID".to_owned(),
            },
            EpisodeOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct EpisodeService {
    source_repo: Arc<dyn SourceRepository>,
    memory_repo: Arc<dyn MemoryPathRepository>,
}
impl EpisodeService {
    pub fn new(
        source_repo: Arc<dyn SourceRepository>,
        memory_repo: Arc<dyn MemoryPathRepository>,
    ) -> Self {
        Self {
            source_repo,
            memory_repo,
        }
    }
    pub async fn create_episode(
        &self,
        title: String,
        source_id_raw: String,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<EpisodeResponse, EpisodeOperationError> {
        let title = EpisodeTitle::new(title).map_err(|_| EpisodeOperationError::BlankTitle)?;
        let source_id =
            SourceId::parse(&source_id_raw).ok_or(EpisodeOperationError::SourceNotFound)?;
        let source = self
            .source_repo
            .get(&source_id)
            .await
            .map_err(|e| EpisodeOperationError::Repository(e.into()))?
            .ok_or(EpisodeOperationError::SourceNotFound)?;
        let source_range =
            SourceRange::new(source_id.clone(), start_byte, end_byte, source.content())
                .map_err(EpisodeOperationError::InvalidRange)?;
        let episode = Episode::new(title, source_range);
        let stored = self
            .memory_repo
            .create_episode(episode)
            .await
            .map_err(|e| EpisodeOperationError::Repository(e.into()))?;
        self.build_response(&stored, &source).await
    }
    pub async fn get_episode(
        &self,
        id: &EpisodeId,
    ) -> Result<EpisodeResponse, EpisodeOperationError> {
        let episode = self
            .memory_repo
            .get_episode(id)
            .await
            .map_err(|e| EpisodeOperationError::Repository(e.into()))?
            .ok_or(EpisodeOperationError::EpisodeNotFound)?;
        let source = self
            .source_repo
            .get(episode.source_range().source_id())
            .await
            .map_err(|e| EpisodeOperationError::Repository(e.into()))?
            .ok_or(EpisodeOperationError::SourceUnavailable)?;
        self.build_response(&episode, &source).await
    }
    async fn build_response(
        &self,
        episode: &Episode,
        source: &lighting_core::Source,
    ) -> Result<EpisodeResponse, EpisodeOperationError> {
        let excerpt = episode.source_range().slice(source.content()).to_owned();
        Ok(EpisodeResponse {
            episode_id: episode.id().as_str().to_owned(),
            title: episode.title().as_str().to_owned(),
            source_id: episode.source_range().source_id().as_str().to_owned(),
            start_byte: episode.source_range().start_byte(),
            end_byte: episode.source_range().end_byte(),
            excerpt,
            created_at: episode.created_at(),
        })
    }
}

#[cfg(test)]
pub fn unavailable_service() -> EpisodeService {
    use lighting_core::{SourceRepositoryError, StoreSourceResult};
    struct UnavailableSourceRepo;
    #[async_trait::async_trait]
    impl SourceRepository for UnavailableSourceRepo {
        async fn store(
            &self,
            _: lighting_core::Source,
        ) -> Result<StoreSourceResult, SourceRepositoryError> {
            Err(SourceRepositoryError::Operation(Box::new(
                std::io::Error::other("unavailable"),
            )))
        }
        async fn get(
            &self,
            _: &lighting_core::SourceId,
        ) -> Result<Option<lighting_core::Source>, SourceRepositoryError> {
            Err(SourceRepositoryError::Operation(Box::new(
                std::io::Error::other("unavailable"),
            )))
        }
    }
    struct UnavailableMemoryRepo;
    #[async_trait::async_trait]
    impl MemoryPathRepository for UnavailableMemoryRepo {
        async fn create_project(
            &self,
            _: lighting_core::Project,
        ) -> Result<lighting_core::Project, lighting_core::MemoryPathRepositoryError> {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn get_project(
            &self,
            _: &lighting_core::ProjectId,
        ) -> Result<Option<lighting_core::Project>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn create_episode(
            &self,
            _: lighting_core::Episode,
        ) -> Result<lighting_core::Episode, lighting_core::MemoryPathRepositoryError> {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn get_episode(
            &self,
            _: &lighting_core::EpisodeId,
        ) -> Result<Option<lighting_core::Episode>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn create_marker(
            &self,
            _: lighting_core::Marker,
        ) -> Result<lighting_core::StoreMarkerResult, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn get_marker(
            &self,
            _: &lighting_core::MarkerId,
        ) -> Result<Option<lighting_core::Marker>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn find_marker_by_lookup(
            &self,
            _: &str,
        ) -> Result<Option<lighting_core::Marker>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn link_episode_project(
            &self,
            _: lighting_core::EpisodeProjectLink,
        ) -> Result<(), lighting_core::MemoryPathRepositoryError> {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn link_episode_marker(
            &self,
            _: lighting_core::EpisodeMarkerLink,
        ) -> Result<(), lighting_core::MemoryPathRepositoryError> {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn list_episode_project_links(
            &self,
            _: &lighting_core::EpisodeId,
        ) -> Result<Vec<lighting_core::EpisodeProjectLink>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn list_episode_marker_links(
            &self,
            _: &lighting_core::EpisodeId,
        ) -> Result<Vec<lighting_core::EpisodeMarkerLink>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn list_marker_episode_links(
            &self,
            _: &lighting_core::MarkerId,
        ) -> Result<Vec<lighting_core::EpisodeMarkerLink>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn list_project_episode_links(
            &self,
            _: &lighting_core::ProjectId,
        ) -> Result<Vec<lighting_core::EpisodeProjectLink>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
        async fn find_project_episode_by_source_range(
            &self,
            _: &lighting_core::ProjectId,
            _: &lighting_core::SourceId,
            _: usize,
            _: usize,
        ) -> Result<Option<lighting_core::Episode>, lighting_core::MemoryPathRepositoryError>
        {
            Err(lighting_core::MemoryPathRepositoryError::Operation(
                Box::new(std::io::Error::other("unavailable")),
            ))
        }
    }
    EpisodeService::new(
        Arc::new(UnavailableSourceRepo),
        Arc::new(UnavailableMemoryRepo),
    )
}
