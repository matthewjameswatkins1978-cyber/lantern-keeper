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
    pub source_id: String,
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
