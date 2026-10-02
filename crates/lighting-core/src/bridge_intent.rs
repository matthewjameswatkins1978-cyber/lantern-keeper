use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durable record of an execution intent processed by the Lantern Bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeIntentRecord {
    pub intent_id: String,
    pub intent_commit: String,
    pub canonical_digest: String,
    pub action: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lantern_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub processed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeClaimResult {
    Claimed {
        record: BridgeIntentRecord,
    },
    Existing {
        record: BridgeIntentRecord,
    },
    Conflict {
        existing_digest: String,
        incoming_digest: String,
    },
}

#[derive(Debug, Error)]
pub enum BridgeIntentRepositoryError {
    #[error("Bridge intent store is unavailable")]
    Unavailable,
    #[error("Bridge intent repository operation failed: {0}")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Bridge intent not found: {0}")]
    NotFound(String),
}

#[async_trait::async_trait]
pub trait BridgeIntentRepository: Send + Sync {
    /// Atomically claim an intent for execution.
    ///
    /// If not present, creates a record in "CLAIMED" status and returns `Claimed`.
    /// If present and digests match, returns `Existing`.
    /// If present but digest differs, returns `Conflict`.
    async fn claim(
        &self,
        intent_id: &str,
        intent_commit: &str,
        canonical_digest: &str,
        action: &str,
    ) -> Result<BridgeClaimResult, BridgeIntentRepositoryError>;

    /// Complete execution of an intent, recording terminal status and result metadata.
    async fn complete(
        &self,
        intent_id: &str,
        status: &str,
        lantern_record_id: Option<String>,
        result_digest: Option<String>,
        error: Option<String>,
    ) -> Result<BridgeIntentRecord, BridgeIntentRepositoryError>;

    /// Get an intent record by ID.
    async fn get(
        &self,
        intent_id: &str,
    ) -> Result<Option<BridgeIntentRecord>, BridgeIntentRepositoryError>;

    /// List all intent records.
    async fn list_all(&self) -> Result<Vec<BridgeIntentRecord>, BridgeIntentRepositoryError>;
}
