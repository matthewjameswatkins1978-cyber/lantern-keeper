//! Application-layer Project operations.

use std::sync::Arc;

use lighting_core::{MemoryPathRepository, Project, ProjectId, ProjectName, ProjectStatus};

use crate::project_dto::ProjectResponse;
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum ProjectOperationError {
    #[error("memory-path repository is not available")]
    Unavailable,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<ProjectOperationError> for ApiError {
    fn from(error: ProjectOperationError) -> Self {
        match error {
            ProjectOperationError::Unavailable => ApiError::storage_unavailable(),
            ProjectOperationError::Repository(_) => ApiError::internal_error(),
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
}
