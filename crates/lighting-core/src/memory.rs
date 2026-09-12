//! Canonical memory domain types and deterministic reconciliation rules.
//!
//! Sources are evidence. Memories are the currently useful interpretation of
//! evidence. This module deliberately contains no database or model-runtime
//! code: persistence and AI-assisted judgement belong outside the domain.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// A stable identifier for a canonical memory.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryId(String);

impl MemoryId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Result<Self, MemoryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(MemoryError::EmptyId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

/// Small, useful vocabulary for durable memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Fact,
    Decision,
    Constraint,
    Preference,
    Rule,
    Idea,
    Task,
    Finding,
    Experience,
    OpenLoop,
}

/// Lifecycle of a canonical memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryState {
    Active,
    Superseded,
    Archived,
    Tombstoned,
    NeedsReview,
}

/// How a memory was handled during reconciliation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReconciliationAction {
    Ignore,
    Forget,
    New,
    Reinforce,
    Correct,
    Supersede,
    Conflict,
    Historical,
    Unresolved,
    Ask,
}

/// A candidate submitted by a curator, importer, or agent runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub scope: Option<String>,
    pub kind: MemoryKind,
    pub canonical_text: String,
    pub identity_key: Option<String>,
    pub confidence: u8,
    pub importance: u8,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub source_ids: Vec<String>,
    pub episode_ids: Vec<String>,
    pub derived_from: Vec<MemoryId>,
    pub actor: String,
    pub sensitivity: String,
    /// Historical evidence is retained but must not enter the current view.
    pub historical: bool,
}

impl MemoryCandidate {
    pub fn normalized_identity(&self) -> String {
        self.identity_key
            .as_deref()
            .unwrap_or(&self.canonical_text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
}

/// Durable interpretation of evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    pub id: MemoryId,
    pub scope: Option<String>,
    pub kind: MemoryKind,
    pub canonical_text: String,
    pub identity_key: String,
    pub state: MemoryState,
    pub confidence: u8,
    pub importance: u8,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_ids: Vec<String>,
    pub episode_ids: Vec<String>,
    pub derived_from: Vec<MemoryId>,
    pub supersedes: Option<MemoryId>,
    pub superseded_by: Option<MemoryId>,
    pub conflicts_with: Vec<MemoryId>,
    pub reinforcement_count: u32,
    pub last_reinforced_at: Option<DateTime<Utc>>,
    pub sensitivity: String,
    pub revision: u32,
    pub checksum: String,
}

impl Memory {
    pub fn from_candidate(candidate: MemoryCandidate, now: DateTime<Utc>) -> Self {
        let identity_key = candidate.normalized_identity();
        let mut memory = Self {
            id: MemoryId::new(),
            scope: candidate.scope,
            kind: candidate.kind,
            canonical_text: candidate.canonical_text,
            identity_key,
            state: if candidate.historical {
                MemoryState::Archived
            } else {
                MemoryState::Active
            },
            confidence: candidate.confidence,
            importance: candidate.importance,
            valid_from: candidate.valid_from,
            valid_to: candidate.valid_to,
            recorded_at: now,
            updated_at: now,
            source_ids: candidate.source_ids,
            episode_ids: candidate.episode_ids,
            derived_from: candidate.derived_from,
            supersedes: None,
            superseded_by: None,
            conflicts_with: vec![],
            reinforcement_count: 1,
            last_reinforced_at: Some(now),
            sensitivity: candidate.sensitivity,
            revision: 1,
            checksum: String::new(),
        };
        memory.checksum = memory.compute_checksum();
        memory
    }

    pub fn compute_checksum(&self) -> String {
        let canonical = format!(
            "{:?}|{:?}|{:?}|{}|{}|{:?}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
            self.id,
            self.scope,
            self.kind,
            self.canonical_text,
            self.identity_key,
            self.state,
            self.confidence,
            self.importance,
            self.valid_from,
            self.valid_to,
            self.source_ids,
            self.episode_ids,
            self.derived_from,
            self.supersedes,
            self.superseded_by,
            self.conflicts_with,
            self.reinforcement_count,
            self.sensitivity,
            self.revision
        );
        format!("{:x}", Sha256::digest(canonical.as_bytes()))
    }
}

/// Outcome of comparing a candidate with existing canonical memories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationDecision {
    pub action: ReconciliationAction,
    pub reason: String,
    pub candidate: MemoryCandidate,
    pub matched_memory_ids: Vec<MemoryId>,
}

/// A typed connection imported from or created alongside a canonical memory.
/// `target_ref` is retained even when the target cannot yet be resolved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub relation_id: String,
    pub from_memory_id: Option<MemoryId>,
    pub to_memory_id: Option<MemoryId>,
    pub relation_type: String,
    pub target_ref: String,
    pub source_id: Option<String>,
}

impl MemoryRelation {
    pub fn new(
        from_memory_id: Option<MemoryId>,
        to_memory_id: Option<MemoryId>,
        relation_type: String,
        target_ref: String,
        source_id: Option<String>,
    ) -> Self {
        Self::with_id(
            uuid::Uuid::new_v4().to_string(),
            from_memory_id,
            to_memory_id,
            relation_type,
            target_ref,
            source_id,
        )
    }

    pub fn with_id(
        relation_id: String,
        from_memory_id: Option<MemoryId>,
        to_memory_id: Option<MemoryId>,
        relation_type: String,
        target_ref: String,
        source_id: Option<String>,
    ) -> Self {
        Self {
            relation_id,
            from_memory_id,
            to_memory_id,
            relation_type,
            target_ref,
            source_id,
        }
    }
}

/// Persistence boundary for canonical memory.
#[async_trait]
pub trait MemoryRepository: Send + Sync {
    async fn list(
        &self,
        scope: Option<&str>,
        include_history: bool,
    ) -> Result<Vec<Memory>, MemoryRepositoryError>;

    async fn get(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError>;

    async fn reconcile_and_apply(
        &self,
        candidate: MemoryCandidate,
    ) -> Result<ReconciliationDecision, MemoryRepositoryError>;

    async fn forget(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError>;
}

/// Persistence boundary for typed memory relations.
#[async_trait]
pub trait MemoryRelationRepository: Send + Sync {
    async fn upsert_relation(
        &self,
        relation: MemoryRelation,
    ) -> Result<MemoryRelation, MemoryRepositoryError>;

    async fn list_relations(
        &self,
        memory_id: Option<&MemoryId>,
    ) -> Result<Vec<MemoryRelation>, MemoryRepositoryError>;
}

#[derive(Debug, Error)]
pub enum MemoryRepositoryError {
    #[error("memory repository operation failed")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Deterministic baseline reconciliation.
pub fn reconcile_candidate(
    candidate: MemoryCandidate,
    existing: &[Memory],
) -> ReconciliationDecision {
    if candidate.canonical_text.trim().is_empty() {
        return ReconciliationDecision {
            action: ReconciliationAction::Ignore,
            reason: "blank candidate".to_owned(),
            candidate,
            matched_memory_ids: vec![],
        };
    }

    let identity = candidate.normalized_identity();
    let explicit_identity = candidate.identity_key.is_some();
    let matches: Vec<&Memory> = existing
        .iter()
        .filter(|memory| {
            memory.state != MemoryState::Tombstoned
                && memory.identity_key == identity
                && (explicit_identity
                    || (memory.scope == candidate.scope && memory.kind == candidate.kind))
        })
        .collect();

    if matches.is_empty() && candidate.historical {
        return ReconciliationDecision {
            action: ReconciliationAction::Historical,
            reason: "candidate is explicitly historical".to_owned(),
            candidate,
            matched_memory_ids: vec![],
        };
    }

    if matches.is_empty() {
        return ReconciliationDecision {
            action: ReconciliationAction::New,
            reason: "no matching canonical memory".to_owned(),
            candidate,
            matched_memory_ids: vec![],
        };
    }

    let ids = matches.iter().map(|memory| memory.id.clone()).collect();
    if matches
        .iter()
        .any(|memory| memory.canonical_text == candidate.canonical_text)
    {
        return ReconciliationDecision {
            action: ReconciliationAction::Reinforce,
            reason: "same proposition observed again".to_owned(),
            candidate,
            matched_memory_ids: ids,
        };
    }

    if candidate.historical {
        return ReconciliationDecision {
            action: ReconciliationAction::Historical,
            reason: "candidate is explicitly historical".to_owned(),
            candidate,
            matched_memory_ids: ids,
        };
    }

    let active = matches
        .iter()
        .find(|memory| memory.state == MemoryState::Active);
    if active.is_some() && candidate.confidence >= 70 {
        return ReconciliationDecision {
            action: ReconciliationAction::Supersede,
            reason: "same identity has a newer or more authoritative proposition".to_owned(),
            candidate,
            matched_memory_ids: ids,
        };
    }

    ReconciliationDecision {
        action: ReconciliationAction::Conflict,
        reason: "matching identity has incompatible or insufficiently authoritative evidence"
            .to_owned(),
        candidate,
        matched_memory_ids: ids,
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryError {
    #[error("memory ID cannot be empty")]
    EmptyId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(text: &str, confidence: u8) -> MemoryCandidate {
        MemoryCandidate {
            scope: Some("Threadmoth".to_owned()),
            kind: MemoryKind::Fact,
            canonical_text: text.to_owned(),
            identity_key: Some("threadmoth.release".to_owned()),
            confidence,
            importance: 80,
            valid_from: None,
            valid_to: None,
            source_ids: vec!["source-1".to_owned()],
            episode_ids: vec![],
            derived_from: vec![],
            actor: "test".to_owned(),
            sensitivity: "normal".to_owned(),
            historical: false,
        }
    }

    #[test]
    fn identical_candidate_reinforces_instead_of_duplicating() {
        let existing = Memory::from_candidate(candidate("Threadmoth is 1.9.1", 90), Utc::now());
        let decision = reconcile_candidate(candidate("Threadmoth is 1.9.1", 90), &[existing]);
        assert_eq!(decision.action, ReconciliationAction::Reinforce);
    }

    #[test]
    fn newer_high_confidence_candidate_supersedes_current_truth() {
        let existing = Memory::from_candidate(candidate("Threadmoth is 1.8.1", 80), Utc::now());
        let decision = reconcile_candidate(candidate("Threadmoth is 1.9.1", 95), &[existing]);
        assert_eq!(decision.action, ReconciliationAction::Supersede);
    }

    #[test]
    fn weak_incompatible_candidate_stays_a_conflict() {
        let existing = Memory::from_candidate(candidate("Threadmoth is 1.8.1", 80), Utc::now());
        let decision = reconcile_candidate(candidate("Threadmoth is 1.9.1", 40), &[existing]);
        assert_eq!(decision.action, ReconciliationAction::Conflict);
    }

    #[test]
    fn explicit_identity_prevents_duplicate_when_metadata_changes() {
        let existing = Memory::from_candidate(candidate("Matthew uses Zed", 90), Utc::now());
        let mut changed = candidate("Matthew uses Zed on Mac", 90);
        changed.kind = MemoryKind::Preference;
        changed.scope = Some("Tooling".to_owned());
        assert_eq!(
            reconcile_candidate(changed, &[existing]).action,
            ReconciliationAction::Supersede
        );
    }

    #[test]
    fn historical_candidate_does_not_enter_current_view() {
        let mut historical = candidate("Threadmoth was 1.8.1", 90);
        historical.historical = true;
        assert_eq!(
            reconcile_candidate(historical, &[]).action,
            ReconciliationAction::Historical
        );
        assert_eq!(
            Memory::from_candidate(candidate("Threadmoth was 1.8.1", 90), Utc::now()).state,
            MemoryState::Active
        );
    }
}
