//! Application-layer Project operations.

use std::sync::Arc;

use lighting_core::{
    Episode, EpisodeProjectLink, EpisodeTitle, MemoryPathRepository, Project, ProjectId,
    ProjectLinkKind, ProjectName, ProjectStatus, SourceContent, SourceRange,
};

use crate::project_dto::{ProjectResponse, RecordResultResponse};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum ProjectOperationError {
    #[error("memory-path repository is not available")]
    Unavailable,
    #[error("project not found")]
    NotFound,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("source operation failed")]
    Source(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<ProjectOperationError> for ApiError {
    fn from(error: ProjectOperationError) -> Self {
        match error {
            ProjectOperationError::Unavailable => ApiError::storage_unavailable(),
            ProjectOperationError::NotFound => ApiError {
                code: "project_not_found".to_owned(),
                message: "No Project exists with the given ID".to_owned(),
            },
            ProjectOperationError::Repository(_) => ApiError::internal_error(),
            ProjectOperationError::Source(_) => ApiError {
                code: "source_unavailable".to_owned(),
                message: "The result Source could not be recorded".to_owned(),
            },
        }
    }
}

#[derive(Clone)]
pub struct ProjectService {
    repo: Arc<dyn MemoryPathRepository>,
}

impl ProjectService {
    pub fn new(repo: Arc<dyn MemoryPathRepository>) -> Self {
        Self { repo }
    }

    pub async fn create_project(
        &self,
        name: String,
        status_str: String,
    ) -> Result<ProjectResponse, ProjectOperationError> {
        let name = ProjectName::new(name).map_err(|_| {
            ProjectOperationError::Repository(Box::new(std::io::Error::other(
                "invalid project name",
            )))
        })?;
        let status = match status_str.as_str() {
            "active" => ProjectStatus::Active,
            "paused" => ProjectStatus::Paused,
            "archived" => ProjectStatus::Archived,
            _ => ProjectStatus::Active,
        };
        let project = Project::new(name, status);
        let result = self
            .repo
            .create_project(project)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;
        Ok(ProjectResponse::from_domain(&result))
    }

    pub async fn get_project(
        &self,
        id: &ProjectId,
    ) -> Result<Option<ProjectResponse>, ProjectOperationError> {
        let project = self
            .repo
            .get_project(id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;
        Ok(project.as_ref().map(ProjectResponse::from_domain))
    }

    pub async fn record_result(
        &self,
        project_id: &ProjectId,
        title: String,
        source_id: lighting_core::SourceId,
        source_content: SourceContent,
    ) -> Result<RecordResultResponse, ProjectOperationError> {
        // Verify project exists
        let _project = self
            .repo
            .get_project(project_id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?
            .ok_or(ProjectOperationError::NotFound)?;

        let content_len = source_content.as_bytes().len();

        // Check idempotency: does this Project already have a linked Episode
        // with this source_id and full byte range?
        if let Some(existing) = self
            .repo
            .find_project_episode_by_source_range(project_id, &source_id, 0, content_len)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?
        {
            return Ok(RecordResultResponse {
                outcome: "already_recorded".to_owned(),
                project_id: project_id.as_str().to_owned(),
                source_id: source_id.as_str().to_owned(),
                episode_id: existing.id().as_str().to_owned(),
                start_byte: 0,
                end_byte: content_len,
            });
        }

        // Create a full-range Episode
        let source_range = SourceRange::new(source_id.clone(), 0, content_len, &source_content)
            .map_err(|e| {
                ProjectOperationError::Repository(Box::new(std::io::Error::other(e.to_string())))
            })?;
        let episode_title = EpisodeTitle::new(&title).map_err(|_| {
            ProjectOperationError::Repository(Box::new(std::io::Error::other(
                "invalid episode title",
            )))
        })?;
        let episode = Episode::new(episode_title, source_range);

        let episode = self
            .repo
            .create_episode(episode)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;

        // Link to Project as primary
        let link = EpisodeProjectLink::new(
            episode.id().clone(),
            project_id.clone(),
            ProjectLinkKind::Primary,
        );
        self.repo
            .link_episode_project(link)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;

        Ok(RecordResultResponse {
            outcome: "recorded".to_owned(),
            project_id: project_id.as_str().to_owned(),
            source_id: source_id.as_str().to_owned(),
            episode_id: episode.id().as_str().to_owned(),
            start_byte: 0,
            end_byte: content_len,
        })
    }
}
