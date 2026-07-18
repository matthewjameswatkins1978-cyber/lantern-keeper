//! Request and response DTOs for the Episode HTTP API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Request body for Episode creation.
#[derive(Debug, Deserialize)]
pub struct CreateEpisodeRequest {
    pub title: String,
    pub source_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl CreateEpisodeRequest {
    /// Basic HTTP-level validation. Domain-level range validation is done
    /// by EpisodeService using the authoritative Source content.
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.title.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_episode".to_owned(),
                message: "Episode title must not be blank".to_owned(),
            });
        }
        if self.source_id.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_episode".to_owned(),
                message: "Source ID must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

/// Response body for an Episode.
#[derive(Debug, Serialize)]
pub struct EpisodeResponse {
    pub episode_id: String,
    pub title: String,
    pub source_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub excerpt: String,
    pub created_at: DateTime<Utc>,
}
