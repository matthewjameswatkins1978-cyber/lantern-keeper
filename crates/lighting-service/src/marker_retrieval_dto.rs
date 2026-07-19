//! Request and response DTOs for Marker-led retrieval.

use serde::{Deserialize, Serialize};

/// Request body for marker-led retrieval.
#[derive(Debug, Deserialize)]
pub struct MarkerRetrievalRequest {
    pub text: String,
}

/// A marker match (null when no exact Marker exists).
#[derive(Debug, Serialize)]
pub struct MarkerMatch {
    pub marker_id: String,
    pub display_text: String,
    pub lookup_key: String,
}

/// An Episode returned by marker-led retrieval.
#[derive(Debug, Serialize)]
pub struct RetrievedEpisode {
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

/// The full marker-led retrieval response.
#[derive(Debug, Serialize)]
pub struct MarkerRetrievalResponse {
    pub query: String,
    pub marker: Option<MarkerMatch>,
    pub episodes: Vec<RetrievedEpisode>,
    pub warnings: Vec<String>,
}
