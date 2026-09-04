//! Project-scoped retrieval operations.
//!
//! Path: Project ID → graph traversal → Episodes → Source excerpts.

use std::sync::Arc;

use lighting_core::{MemoryPathRepository, ProjectId, SourceRepository, find_all_matches};

use crate::project_retrieval_dto::{
    ProjectContextPackage, ProjectRetrievalResponse, ProjectRetrievedEpisode, ProjectSummary,
};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum ProjectRetrievalError {
    #[error("storage unavailable")]
    Unavailable,
    #[error("project not found")]
    NotFound,
    #[error("the Source referenced by a linked Episode cannot be loaded")]
    SourceUnavailable,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<ProjectRetrievalError> for ApiError {
    fn from(error: ProjectRetrievalError) -> Self {
        match error {
            ProjectRetrievalError::Unavailable => ApiError::storage_unavailable(),
            ProjectRetrievalError::NotFound => ApiError {
                code: "project_not_found".to_owned(),
                message: "No Project exists with the given ID".to_owned(),
            },
            ProjectRetrievalError::SourceUnavailable => ApiError {
                code: "source_unavailable".to_owned(),
                message: "The Source referenced by a linked Episode cannot be loaded".to_owned(),
            },
            ProjectRetrievalError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct ProjectRetrievalService {
    memory_repo: Arc<dyn MemoryPathRepository>,
    source_repo: Arc<dyn SourceRepository>,
}

impl ProjectRetrievalService {
    pub fn new(
        memory_repo: Arc<dyn MemoryPathRepository>,
        source_repo: Arc<dyn SourceRepository>,
    ) -> Self {
        Self {
            memory_repo,
            source_repo,
        }
    }

    pub async fn retrieve(
        &self,
        project_id_raw: &str,
    ) -> Result<ProjectRetrievalResponse, ProjectRetrievalError> {
        let project_id =
            ProjectId::new(project_id_raw).map_err(|_| ProjectRetrievalError::NotFound)?;

        let project = self
            .memory_repo
            .get_project(&project_id)
            .await
            .map_err(|e| ProjectRetrievalError::Repository(e.into()))?
            .ok_or(ProjectRetrievalError::NotFound)?;

        let project_summary = ProjectSummary {
            project_id: project.id().as_str().to_owned(),
            name: project.name().as_str().to_owned(),
            status: status_to_str(project.status()).to_owned(),
            created_at: project.created_at(),
        };

        let links = self
            .memory_repo
            .list_project_episode_links(&project_id)
            .await
            .map_err(|e| ProjectRetrievalError::Repository(e.into()))?;

        if links.is_empty() {
            let context_package = build_context_package(&project_summary, &[]);
            return Ok(ProjectRetrievalResponse {
                project: project_summary,
                episodes: vec![],
                context_package,
                warnings: vec!["Project exists but is not linked to any Episodes.".to_owned()],
            });
        }

        let mut episodes = Vec::with_capacity(links.len());
        for link in &links {
            let episode = match self
                .memory_repo
                .get_episode(link.episode_id())
                .await
                .map_err(|e| ProjectRetrievalError::Repository(e.into()))?
            {
                Some(ep) => ep,
                None => continue,
            };

            let source = self
                .source_repo
                .get(episode.source_range().source_id())
                .await
                .map_err(|e| ProjectRetrievalError::Repository(e.into()))?
                .ok_or(ProjectRetrievalError::SourceUnavailable)?;

            // Extract the historical excerpt from the original source.
            let historical_excerpt = episode.source_range().slice(source.content()).to_owned();

            // Resolve the current revision for the handoff view.
            let current_revision = self
                .source_repo
                .get_current(source.kind(), source.title())
                .await
                .map_err(|e| ProjectRetrievalError::Repository(e.into()))?;

            let hist_start = episode.source_range().start_byte();
            let hist_end = episode.source_range().end_byte();
            let hist_len = historical_excerpt.len();

            let (content_source_id, latest_source_id, excerpt, start_byte, end_byte) =
                if let Some(ref current) = current_revision {
                    if current.id() == source.id() {
                        // No newer revision — use historical as-is.
                        (
                            source.id().as_str().to_owned(),
                            None,
                            historical_excerpt,
                            hist_start,
                            hist_end,
                        )
                    } else {
                        // Attempt exact-text rebasing.
                        let current_content = current.content().as_str();
                        let matches = find_all_matches(current_content, &historical_excerpt);

                        if matches.len() == 1 {
                            let pos = matches[0];
                            (
                                current.id().as_str().to_owned(),
                                None,
                                historical_excerpt,
                                pos,
                                pos + hist_len,
                            )
                        } else {
                            // Cannot safely rebase — retain historical.
                            (
                                source.id().as_str().to_owned(),
                                Some(current.id().as_str().to_owned()),
                                historical_excerpt,
                                hist_start,
                                hist_end,
                            )
                        }
                    }
                } else {
                    // No current revision at all — use historical.
                    (
                        source.id().as_str().to_owned(),
                        None,
                        historical_excerpt,
                        hist_start,
                        hist_end,
                    )
                };

            let why_matched = format!(
                "Episode is linked to Project \"{}\".",
                project.name().as_str()
            );

            episodes.push(ProjectRetrievedEpisode {
                episode_id: episode.id().as_str().to_owned(),
                title: episode.title().as_str().to_owned(),
                source_id: episode.source_range().source_id().as_str().to_owned(),
                content_source_id,
                latest_source_id,
                start_byte,
                end_byte,
                excerpt,
                why_matched,
            });
        }

        episodes.sort_by(|a, b| a.episode_id.cmp(&b.episode_id));
        let context_package = build_context_package(&project_summary, &episodes);

        Ok(ProjectRetrievalResponse {
            project: project_summary,
            episodes,
            context_package,
            warnings: vec![],
        })
    }
}

fn build_context_package(
    project: &ProjectSummary,
    episodes: &[ProjectRetrievedEpisode],
) -> ProjectContextPackage {
    let mut content = String::new();
    content.push_str("# Codex Handoff Context\n\n");
    content.push_str("## Project\n");
    content.push_str(&format!(
        "- Name: {}\n- ID: {}\n- Status: {}\n\n",
        project.name, project.project_id, project.status
    ));

    if episodes.is_empty() {
        content.push_str("## Relevant Source Episodes\n");
        content.push_str("No Episodes are currently linked to this Project.\n");
    } else {
        content.push_str("## Relevant Source Episodes\n");
        for (index, episode) in episodes.iter().enumerate() {
            content.push_str(&format!("\n### {}. {}\n", index + 1, episode.title));
            content.push_str(&format!("- Episode ID: {}\n", episode.episode_id));
            content.push_str(&format!("- Why: {}\n", episode.why_matched));
            content.push_str(&format!(
                "- Historical Source (Episode reference): {}\n",
                episode.source_id
            ));
            content.push_str(&format!(
                "- Content Source (excerpt origin): {}, bytes {}..{}\n",
                episode.content_source_id, episode.start_byte, episode.end_byte
            ));
            content.push_str("\n```text\n");
            content.push_str(&episode.excerpt);
            if !episode.excerpt.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("```\n");
        }
    }

    ProjectContextPackage {
        format: "markdown".to_owned(),
        audience: "codex".to_owned(),
        content,
    }
}

fn status_to_str(s: lighting_core::ProjectStatus) -> &'static str {
    match s {
        lighting_core::ProjectStatus::Active => "active",
        lighting_core::ProjectStatus::Paused => "paused",
        lighting_core::ProjectStatus::Archived => "archived",
    }
}
