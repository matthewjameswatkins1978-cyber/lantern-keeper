//! Application-layer Episode-association operations.
//!
//! Uses `MemoryPathRepository` only. Contains no SurrealDB types, queries,
//! record IDs, edge-table names, or duplicate-handling logic.

use std::sync::Arc;

use lighting_core::{
    EpisodeId, EpisodeMarkerLink, EpisodeProjectLink, MarkerId, MemoryPathRepository, ProjectId,
    ProjectLinkKind,
};

use crate::episode_association_dto::{
    EpisodeMarkerLinksResponse, EpisodeProjectLinksResponse, MarkerLinkEntry, ProjectLinkEntry,
};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum AssociationOperationError {
    #[error("memory-path repository is not available")]
    Unavailable,
    #[error("referenced Episode does not exist")]
    EpisodeNotFound,
    #[error("referenced Project does not exist")]
    ProjectNotFound,
    #[error("referenced Marker does not exist")]
    MarkerNotFound,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<AssociationOperationError> for ApiError {
    fn from(error: AssociationOperationError) -> Self {
        match error {
            AssociationOperationError::Unavailable => ApiError::storage_unavailable(),
            AssociationOperationError::EpisodeNotFound => ApiError {
                code: "episode_not_found".to_owned(),
                message: "No Episode exists with the given ID".to_owned(),
            },
            AssociationOperationError::ProjectNotFound => ApiError {
                code: "project_not_found".to_owned(),
                message: "No Project exists with the given ID".to_owned(),
            },
            AssociationOperationError::MarkerNotFound => ApiError {
                code: "marker_not_found".to_owned(),
                message: "No Marker exists with the given ID".to_owned(),
            },
            AssociationOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct EpisodeAssociationService {
    repo: Arc<dyn MemoryPathRepository>,
}

impl EpisodeAssociationService {
    pub fn new(repo: Arc<dyn MemoryPathRepository>) -> Self {
        Self { repo }
    }

    /// Link an Episode to a Project. Idempotent — duplicate links are
    /// harmless 204.
    pub async fn link_project(
        &self,
        episode_id_raw: &str,
        project_id_raw: &str,
        kind_str: &str,
    ) -> Result<(), AssociationOperationError> {
        let episode_id = EpisodeId::new(episode_id_raw)
            .map_err(|_| AssociationOperationError::EpisodeNotFound)?;
        let project_id = ProjectId::new(project_id_raw)
            .map_err(|_| AssociationOperationError::ProjectNotFound)?;
        let kind = match kind_str {
            "primary" => ProjectLinkKind::Primary,
            "secondary" => ProjectLinkKind::Secondary,
            "possible" => ProjectLinkKind::Possible,
            _ => {
                return Err(AssociationOperationError::ProjectNotFound);
            }
        };

        // Verify Episode exists
        let episode = self
            .repo
            .get_episode(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if episode.is_none() {
            return Err(AssociationOperationError::EpisodeNotFound);
        }

        // Verify Project exists
        let project = self
            .repo
            .get_project(&project_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if project.is_none() {
            return Err(AssociationOperationError::ProjectNotFound);
        }

        let link = EpisodeProjectLink::new(episode_id, project_id, kind);
        self.repo
            .link_episode_project(link)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;

        Ok(())
    }

    /// List all Project links for an Episode.
    pub async fn list_project_links(
        &self,
        episode_id_raw: &str,
    ) -> Result<EpisodeProjectLinksResponse, AssociationOperationError> {
        let episode_id = EpisodeId::new(episode_id_raw)
            .map_err(|_| AssociationOperationError::EpisodeNotFound)?;

        // Verify Episode exists
        let episode = self
            .repo
            .get_episode(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if episode.is_none() {
            return Err(AssociationOperationError::EpisodeNotFound);
        }

        let links = self
            .repo
            .list_episode_project_links(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;

        let projects: Vec<ProjectLinkEntry> = links
            .into_iter()
            .map(|link| ProjectLinkEntry {
                project_id: link.project_id().as_str().to_owned(),
                kind: kind_to_str(link.kind()).to_owned(),
            })
            .collect();

        Ok(EpisodeProjectLinksResponse {
            episode_id: episode_id_raw.to_owned(),
            projects,
        })
    }

    /// Link an Episode to a Marker. Idempotent.
    pub async fn link_marker(
        &self,
        episode_id_raw: &str,
        marker_id_raw: &str,
    ) -> Result<(), AssociationOperationError> {
        let episode_id = EpisodeId::new(episode_id_raw)
            .map_err(|_| AssociationOperationError::EpisodeNotFound)?;
        let marker_id =
            MarkerId::new(marker_id_raw).map_err(|_| AssociationOperationError::MarkerNotFound)?;

        // Verify Episode exists
        let episode = self
            .repo
            .get_episode(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if episode.is_none() {
            return Err(AssociationOperationError::EpisodeNotFound);
        }

        // Verify Marker exists
        let marker = self
            .repo
            .get_marker(&marker_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if marker.is_none() {
            return Err(AssociationOperationError::MarkerNotFound);
        }

        let link = EpisodeMarkerLink::new(episode_id, marker_id);
        self.repo
            .link_episode_marker(link)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;

        Ok(())
    }

    /// List all Marker links for an Episode.
    pub async fn list_marker_links(
        &self,
        episode_id_raw: &str,
    ) -> Result<EpisodeMarkerLinksResponse, AssociationOperationError> {
        let episode_id = EpisodeId::new(episode_id_raw)
            .map_err(|_| AssociationOperationError::EpisodeNotFound)?;

        // Verify Episode exists
        let episode = self
            .repo
            .get_episode(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;
        if episode.is_none() {
            return Err(AssociationOperationError::EpisodeNotFound);
        }

        let links = self
            .repo
            .list_episode_marker_links(&episode_id)
            .await
            .map_err(|e| AssociationOperationError::Repository(e.into()))?;

        let markers: Vec<MarkerLinkEntry> = links
            .into_iter()
            .map(|link| MarkerLinkEntry {
                marker_id: link.marker_id().as_str().to_owned(),
            })
            .collect();

        Ok(EpisodeMarkerLinksResponse {
            episode_id: episode_id_raw.to_owned(),
            markers,
        })
    }
}

fn kind_to_str(k: ProjectLinkKind) -> &'static str {
    match k {
        ProjectLinkKind::Primary => "primary",
        ProjectLinkKind::Secondary => "secondary",
        ProjectLinkKind::Possible => "possible",
    }
}
