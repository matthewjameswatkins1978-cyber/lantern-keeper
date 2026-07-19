//! Request and response Data Transfer Objects for the Source HTTP API.
//!
//! Owned by the service/API layer, not by the domain or SurrealDB adapter.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use lighting_core::{Source, SourceId, SourceKind};

/// The response envelope for a Source creation.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "outcome")]
pub enum CreateSourceResponse {
    #[serde(rename = "stored")]
    Stored {
        source_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_source_id: Option<String>,
    },
    #[serde(rename = "duplicate")]
    Duplicate { source_id: String },
}

/// The response body for a retrieved Source.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceResponse {
    pub source_id: String,
    pub title: String,
    pub kind: String,
    pub content: String,
    pub fingerprint: String,
    pub created_at: DateTime<Utc>,
}

/// Stable API error shape.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

/// The expected JSON request body for Source creation.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CreateSourceRequest {
    pub title: String,
    pub kind: String,
    pub content: String,
}

impl CreateSourceRequest {
    /// Validates the request and returns the canonical [`SourceKind`] if valid.
    pub fn validate(&self) -> Result<SourceKind, ApiError> {
        if self.title.trim().is_empty() {
            return Err(ApiError {
                code: "invalid_source".to_owned(),
                message: "Source title must not be empty".to_owned(),
            });
        }

        if self.content.is_empty() {
            return Err(ApiError {
                code: "invalid_source".to_owned(),
                message: "Source content must not be empty".to_owned(),
            });
        }

        match self.kind.as_str() {
            "markdown" => Ok(SourceKind::Markdown),
            "plain_text" => Ok(SourceKind::PlainText),
            other => Err(ApiError {
                code: "invalid_source".to_owned(),
                message: format!("unsupported source kind: {other}"),
            }),
        }
    }
}

impl SourceResponse {
    pub fn from_domain(source: &Source) -> Self {
        Self {
            source_id: source.id().as_str().to_owned(),
            title: source.title().as_str().to_owned(),
            kind: source.kind().api_name().to_owned(),
            content: source.content().as_str().to_owned(),
            fingerprint: source.fingerprint().as_str().to_owned(),
            created_at: source.created_at(),
        }
    }
}

impl CreateSourceResponse {
    pub fn stored(id: &SourceId, previous_id: Option<&SourceId>) -> Self {
        Self::Stored {
            source_id: id.as_str().to_owned(),
            previous_source_id: previous_id.map(|p| p.as_str().to_owned()),
        }
    }

    pub fn duplicate(id: &SourceId) -> Self {
        Self::Duplicate {
            source_id: id.as_str().to_owned(),
        }
    }
}

impl ApiError {
    pub fn invalid_source_id() -> Self {
        Self {
            code: "invalid_source_id".to_owned(),
            message: "Source ID must be a valid UUID".to_owned(),
        }
    }

    pub fn not_found() -> Self {
        Self {
            code: "source_not_found".to_owned(),
            message: "No Source exists with the given ID".to_owned(),
        }
    }

    pub fn storage_unavailable() -> Self {
        Self {
            code: "storage_unavailable".to_owned(),
            message: "Source storage is not available".to_owned(),
        }
    }

    pub fn internal_error() -> Self {
        Self {
            code: "internal_error".to_owned(),
            message: "An unexpected internal error occurred".to_owned(),
        }
    }

    pub fn payload_too_large() -> Self {
        Self {
            code: "payload_too_large".to_owned(),
            message: "Request body exceeds the maximum allowed size".to_owned(),
        }
    }
}

/// A single revision in the history response.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceHistoryItem {
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_source_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub current: bool,
}

/// Response for listing all revisions of a logical Source.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceHistoryResponse {
    pub title: String,
    pub kind: String,
    pub revisions: Vec<SourceHistoryItem>,
}

/// Trait so we can go from SourceKind to the API wire representation.
trait SourceKindApi {
    fn api_name(&self) -> &str;
}

impl SourceKindApi for SourceKind {
    fn api_name(&self) -> &str {
        match self {
            SourceKind::PlainText => "plain_text",
            SourceKind::Markdown => "markdown",
        }
    }
}
