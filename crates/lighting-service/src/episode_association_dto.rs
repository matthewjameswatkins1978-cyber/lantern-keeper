//! Request and response DTOs for Episode association endpoints.

use serde::{Deserialize, Serialize};

/// Request body for linking an Episode to a Project.
#[derive(Debug, Deserialize)]
pub struct LinkProjectRequest {
    pub project_id: String,
    pub kind: String,
}

impl LinkProjectRequest {
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.project_id.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_project_id".to_owned(),
                message: "Project ID must not be empty".to_owned(),
            });
        }
        match self.kind.as_str() {
            "primary" | "secondary" | "possible" => Ok(()),
            other => Err(super::source_dto::ApiError {
                code: "invalid_link_kind".to_owned(),
                message: format!(
                    "invalid link kind: {other}, expected primary, secondary, or possible"
                ),
            }),
        }
    }
}

/// Request body for linking an Episode to a Marker.
#[derive(Debug, Deserialize)]
pub struct LinkMarkerRequest {
    pub marker_id: String,
}

impl LinkMarkerRequest {
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.marker_id.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_marker_id".to_owned(),
                message: "Marker ID must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

/// Response for listing Episode → Project links.
#[derive(Debug, Serialize)]
pub struct EpisodeProjectLinksResponse {
    pub episode_id: String,
    pub projects: Vec<ProjectLinkEntry>,
}

#[derive(Debug, Serialize)]
pub struct ProjectLinkEntry {
    pub project_id: String,
    pub kind: String,
}

/// Response for listing Episode → Marker links.
#[derive(Debug, Serialize)]
pub struct EpisodeMarkerLinksResponse {
    pub episode_id: String,
    pub markers: Vec<MarkerLinkEntry>,
}

#[derive(Debug, Serialize)]
pub struct MarkerLinkEntry {
    pub marker_id: String,
}
