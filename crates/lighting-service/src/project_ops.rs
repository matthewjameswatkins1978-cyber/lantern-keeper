//! Application-layer Project operations.

use std::sync::Arc;

use lighting_core::{
    Episode, EpisodeProjectLink, EpisodeTitle, MemoryPathRepository, Project, ProjectId,
    ProjectLinkKind, ProjectName, ProjectStatus, SourceContent, SourceId, SourceKind, SourceRange,
    SourceRepository, SourceTitle,
};

use crate::project_dto::{
    AddFileResponse, ProjectResponse, ProjectShowEpisodeEntry, ProjectShowResponse,
    RecordResultResponse,
};
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
    source_repo: Arc<dyn SourceRepository>,
}

impl ProjectService {
    pub fn new(
        repo: Arc<dyn MemoryPathRepository>,
        source_repo: Arc<dyn SourceRepository>,
    ) -> Self {
        Self { repo, source_repo }
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

    /// Add a local file to a Project.
    ///
    /// This is the core operation behind `project-add-file`.
    /// It captures the file content as a Source (with fingerprint dedup),
    /// creates or reuses a full-range Episode, and links it to the Project.
    /// All three steps are idempotent via logical Source identity `(kind, title)`.
    pub async fn add_file(
        &self,
        project_id: &ProjectId,
        title: String,
        kind: SourceKind,
        source_id: SourceId,
        source_content: SourceContent,
    ) -> Result<AddFileResponse, ProjectOperationError> {
        // Verify project exists
        let _project = self
            .repo
            .get_project(project_id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?
            .ok_or(ProjectOperationError::NotFound)?;

        // Check idempotency by logical Source identity `(kind, title)`:
        // does this Project already have a linked Episode whose Source
        // shares the same kind+title (i.e. is the same logical file)?
        let logical_title = SourceTitle::new(&title).map_err(|_| {
            ProjectOperationError::Repository(Box::new(std::io::Error::other(
                "invalid source title",
            )))
        })?;

        let links = self
            .repo
            .list_project_episode_links(project_id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;

        let mut existing_episode: Option<(Episode, SourceId)> = None;
        for link in &links {
            if let Ok(Some(ep)) = self
                .repo
                .get_episode(link.episode_id())
                .await
                .map_err(|e| ProjectOperationError::Repository(e.into()))
            {
                let ep_source_id = ep.source_range().source_id().clone();
                if let Ok(Some(existing_source)) = self
                    .source_repo
                    .get(&ep_source_id)
                    .await
                    .map_err(|e| ProjectOperationError::Repository(e.into()))
                {
                    if existing_source.kind() == kind
                        && existing_source.title().as_str() == logical_title.as_str()
                    {
                        existing_episode = Some((ep, ep_source_id));
                        break;
                    }
                }
            }
        }

        if let Some((existing_ep, existing_source_id)) = existing_episode {
            // This Project already has an Episode for this logical file.
            // If the source content is unchanged (same source_id), it's a
            // pure duplicate. If the source_id differs, the Source was
            // revised but no new Project linkage is needed.
            if existing_source_id == source_id {
                return Ok(AddFileResponse {
                    outcome: "unchanged".to_owned(),
                    source_id: source_id.as_str().to_owned(),
                    previous_source_id: None,
                    episode_id: existing_ep.id().as_str().to_owned(),
                    link_status: "already_linked".to_owned(),
                });
            } else {
                return Ok(AddFileResponse {
                    outcome: "revision_captured_existing_project_link".to_owned(),
                    source_id: source_id.as_str().to_owned(),
                    previous_source_id: None,
                    episode_id: existing_ep.id().as_str().to_owned(),
                    link_status: "already_linked".to_owned(),
                });
            }
        }

        let content_len = source_content.as_bytes().len();

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

        Ok(AddFileResponse {
            outcome: "linked".to_owned(),
            source_id: source_id.as_str().to_owned(),
            previous_source_id: None,
            episode_id: episode.id().as_str().to_owned(),
            link_status: "linked".to_owned(),
        })
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

    pub async fn list_projects(&self) -> Result<Vec<ProjectResponse>, ProjectOperationError> {
        let projects = self
            .repo
            .list_all_projects()
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;
        Ok(projects.iter().map(ProjectResponse::from_domain).collect())
    }

    /// Inspect a single Project with its linked Episodes and Source provenance.
    pub async fn show(
        &self,
        project_id: &ProjectId,
    ) -> Result<ProjectShowResponse, ProjectOperationError> {
        let project = self
            .repo
            .get_project(project_id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?
            .ok_or(ProjectOperationError::NotFound)?;

        let links = self
            .repo
            .list_project_episode_links(project_id)
            .await
            .map_err(|e| ProjectOperationError::Repository(e.into()))?;

        let mut episodes = Vec::with_capacity(links.len());
        for link in &links {
            let episode = match self
                .repo
                .get_episode(link.episode_id())
                .await
                .map_err(|e| ProjectOperationError::Repository(e.into()))
            {
                Ok(Some(ep)) => ep,
                Ok(None) => continue,
                Err(_) => continue,
            };

            let source_id = episode.source_range().source_id().clone();
            let source = match self
                .source_repo
                .get(&source_id)
                .await
                .map_err(|e| ProjectOperationError::Source(Box::new(e)))
            {
                Ok(Some(s)) => s,
                Ok(None) => continue,
                Err(_) => continue,
            };

            // If a newer revision exists, expose it without changing the
            // Episode's historical source_id.
            let latest_source_id = self
                .source_repo
                .get_current(source.kind(), source.title())
                .await
                .map_err(|e| ProjectOperationError::Source(Box::new(e)))
                .ok()
                .flatten()
                .filter(|latest| latest.id() != &source_id)
                .map(|latest| latest.id().as_str().to_owned());

            let kind_str = match source.kind() {
                lighting_core::SourceKind::Markdown => "markdown",
                lighting_core::SourceKind::PlainText => "plain_text",
            };

            episodes.push(ProjectShowEpisodeEntry {
                episode_id: episode.id().as_str().to_owned(),
                link_kind: link_kind_to_str(&link.kind()).to_owned(),
                source_id: source_id.as_str().to_owned(),
                start_byte: episode.source_range().start_byte(),
                end_byte: episode.source_range().end_byte(),
                source_title: source.title().as_str().to_owned(),
                source_kind: kind_str.to_owned(),
                latest_source_id,
                created_at: episode.created_at(),
            });
        }

        // Deterministic ordering: created_at ASC, then episode_id ASC.
        episodes.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.episode_id.cmp(&b.episode_id))
        });

        Ok(ProjectShowResponse {
            project_id: project.id().as_str().to_owned(),
            name: project.name().as_str().to_owned(),
            status: status_to_str(project.status()).to_owned(),
            created_at: project.created_at(),
            episodes,
        })
    }
}

fn status_to_str(s: lighting_core::ProjectStatus) -> &'static str {
    match s {
        lighting_core::ProjectStatus::Active => "active",
        lighting_core::ProjectStatus::Paused => "paused",
        lighting_core::ProjectStatus::Archived => "archived",
    }
}

fn link_kind_to_str(kind: &lighting_core::ProjectLinkKind) -> &'static str {
    match kind {
        lighting_core::ProjectLinkKind::Primary => "primary",
        lighting_core::ProjectLinkKind::Secondary => "secondary",
        lighting_core::ProjectLinkKind::Possible => "possible",
    }
}
