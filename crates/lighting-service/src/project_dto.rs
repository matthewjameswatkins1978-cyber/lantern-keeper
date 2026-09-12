//! Request and response DTOs for the Project HTTP API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use lighting_core::Project;

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "active".to_owned()
}

impl CreateProjectRequest {
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.name.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_project".to_owned(),
                message: "Project name must not be empty".to_owned(),
            });
        }
        match self.status.as_str() {
            "active" | "paused" | "archived" => Ok(()),
            other => Err(super::source_dto::ApiError {
                code: "invalid_project".to_owned(),
                message: format!("invalid project status: {other}"),
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl ProjectResponse {
    pub fn from_domain(project: &Project) -> Self {
        Self {
            project_id: project.id().as_str().to_owned(),
            name: project.name().as_str().to_owned(),
            status: status_to_str(project.status()).to_owned(),
            created_at: project.created_at(),
        }
    }
}

fn status_to_str(s: lighting_core::ProjectStatus) -> &'static str {
    match s {
        lighting_core::ProjectStatus::Active => "active",
        lighting_core::ProjectStatus::Paused => "paused",
        lighting_core::ProjectStatus::Archived => "archived",
    }
}

// ── Record-result DTOs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RecordResultRequest {
    pub title: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub content: String,
}

fn default_kind() -> String {
    "markdown".to_owned()
}

impl RecordResultRequest {
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.title.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_result".to_owned(),
                message: "result title must not be empty".to_owned(),
            });
        }
        if self.content.is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_result".to_owned(),
                message: "result content must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct RecordResultResponse {
    pub outcome: String,
    pub project_id: String,
    pub source_id: String,
    pub episode_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

// ── Add-file DTOs ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddFileRequest {
    pub title: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub content: String,
}

impl AddFileRequest {
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.title.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_add_file".to_owned(),
                message: "title must not be empty".to_owned(),
            });
        }
        if self.content.is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_add_file".to_owned(),
                message: "file content must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct AddFileResponse {
    pub outcome: String,
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_source_id: Option<String>,
    pub episode_id: String,
    pub link_status: String,
}

// ── Project-show response ──────────────────────────────────────────────

/// A single Episode entry in the Project show view.
#[derive(Debug, Serialize)]
pub struct ProjectShowEpisodeEntry {
    pub episode_id: String,
    pub link_kind: String,
    pub source_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub source_title: String,
    pub source_kind: String,
    /// Present only when the Source has a newer revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_source_id: Option<String>,
    #[serde(skip)]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Full Project inspection view.
#[derive(Debug, Serialize)]
pub struct ProjectShowResponse {
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub episodes: Vec<ProjectShowEpisodeEntry>,
}
