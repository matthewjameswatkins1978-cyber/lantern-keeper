//! Host-neutral append-only events for the source ledger.
//!
//! An event is evidence supplied by a host or adapter. It is not itself a
//! derived Memory, and replaying the same event must be safe.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerRole {
    User,
    Assistant,
    System,
    Tool,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEvent {
    /// Stable Lantern identity for this event within the source ledger.
    pub event_id: String,
    /// Adapter or host that supplied the event, for example `lucy` or `codex`.
    pub source: String,
    pub external_id: Option<String>,
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub actor: String,
    pub role: LedgerRole,
    pub content: String,
    pub observed_at: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub reply_to: Option<String>,
    pub project_hint: Option<String>,
    /// Stable replay key. It may be an upstream event ID or an adapter-derived
    /// hash, but it must remain stable across process restarts.
    pub idempotency_key: String,
    /// Exact source payload, when the adapter can retain it without interpretation.
    pub raw_payload: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Field-specific validation errors are clearer at the API boundary.
pub enum LedgerEventError {
    #[error("ledger event ID must not be blank")]
    EventIdBlank,
    #[error("ledger event source must not be blank")]
    SourceBlank,
    #[error("ledger event actor must not be blank")]
    ActorBlank,
    #[error("ledger event content must not be blank")]
    ContentBlank,
    #[error("ledger event idempotency key must not be blank")]
    IdempotencyKeyBlank,
}

impl LedgerEvent {
    pub fn validate(&self) -> Result<(), LedgerEventError> {
        if self.event_id.trim().is_empty() {
            return Err(LedgerEventError::EventIdBlank);
        }
        if self.source.trim().is_empty() {
            return Err(LedgerEventError::SourceBlank);
        }
        if self.actor.trim().is_empty() {
            return Err(LedgerEventError::ActorBlank);
        }
        if self.content.trim().is_empty() {
            return Err(LedgerEventError::ContentBlank);
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(LedgerEventError::IdempotencyKeyBlank);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerIngestResult {
    Stored(LedgerEvent),
    Duplicate(LedgerEvent),
}

#[derive(Debug, Error)]
pub enum LedgerRepositoryError {
    #[error("ledger repository operation failed: {0}")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[async_trait::async_trait]
pub trait LedgerEventRepository: Send + Sync {
    async fn ingest(
        &self,
        event: LedgerEvent,
    ) -> Result<LedgerIngestResult, LedgerRepositoryError>;

    async fn list(&self, source: Option<&str>) -> Result<Vec<LedgerEvent>, LedgerRepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_validation_keeps_identity_and_replay_requirements_explicit() {
        let event = LedgerEvent {
            event_id: "lucy:turn:1".to_owned(),
            source: "lucy".to_owned(),
            external_id: Some("turn-1".to_owned()),
            session_id: Some("session-1".to_owned()),
            conversation_id: Some("conversation-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            actor: "matthew".to_owned(),
            role: LedgerRole::User,
            content: "Remember this.".to_owned(),
            observed_at: None,
            received_at: Utc::now(),
            reply_to: None,
            project_hint: None,
            idempotency_key: "lucy:turn:1".to_owned(),
            raw_payload: Some("{\"content\":\"Remember this.\"}".to_owned()),
            metadata: BTreeMap::new(),
        };

        assert_eq!(event.validate(), Ok(()));
        let mut invalid = event;
        invalid.idempotency_key.clear();
        assert_eq!(invalid.validate(), Err(LedgerEventError::IdempotencyKeyBlank));
    }
}
