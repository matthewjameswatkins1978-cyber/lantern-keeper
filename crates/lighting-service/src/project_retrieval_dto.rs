//! Request and response DTOs for project-scoped retrieval.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Request body for project-scoped retrieval.
#[derive(Debug, Deserialize)]
pub struct ProjectRetrievalRequest {
    pub project_id: String,
}

/// A Project summary in the retrieval response.
#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// An Episode returned by project-scoped retrieval.
#[derive(Debug, Serialize)]
pub struct ProjectRetrievedEpisode {
    pub episode_id: String,
    pub title: String,
    /// The historically-referenced Source ID (the one the Episode was created with).
    pub source_id: String,
    /// The exact Source whose content was used to produce the excerpt.
    /// Always present; equal to `source_id` when no newer revision exists.
    pub content_source_id: String,
    /// When a newer Source revision exists but the excerpt could not be safely
    /// rebased into it, this identifies that revision.  Absent when the
    /// current revision was used or no revision exists.
    pub latest_source_id: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub excerpt: String,
    pub why_matched: String,
}

/// A compact context package intended for a coding agent handoff.
#[derive(Debug, Serialize)]
pub struct ProjectContextPackage {
    pub format: String,
    pub audience: String,
    pub content: String,
}

/// The full project-scoped retrieval response.
#[derive(Debug, Serialize)]
pub struct ProjectRetrievalResponse {
    pub project: ProjectSummary,
    pub episodes: Vec<ProjectRetrievedEpisode>,
    pub context_package: ProjectContextPackage,
    pub warnings: Vec<String>,
}
