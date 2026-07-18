//! Request and response DTOs for the Marker HTTP API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use lighting_core::Marker;

/// Request body for Marker creation.
#[derive(Debug, Deserialize)]
pub struct CreateMarkerRequest {
    pub text: String,
}

impl CreateMarkerRequest {
    /// Validates the request. Returns an ApiError if the text is blank.
    pub fn validate(&self) -> Result<(), super::source_dto::ApiError> {
        if self.text.trim().is_empty() {
            return Err(super::source_dto::ApiError {
                code: "invalid_marker".to_owned(),
                message: "Marker text must not be blank".to_owned(),
            });
        }
        Ok(())
    }
}

/// Response body for a Marker.
#[derive(Debug, Serialize)]
pub struct MarkerResponse {
    pub marker_id: String,
    pub display_text: String,
    pub lookup_key: String,
    pub created_at: DateTime<Utc>,
}

impl MarkerResponse {
    pub fn from_domain(marker: &Marker) -> Self {
        Self {
            marker_id: marker.id().as_str().to_owned(),
            display_text: marker.display_text().to_owned(),
            lookup_key: marker.lookup_key().to_owned(),
            created_at: marker.created_at(),
        }
    }
}

/// Query parameters for the lookup endpoint.
#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    pub text: String,
}
