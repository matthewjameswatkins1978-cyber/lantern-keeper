use chrono::{DateTime, Utc};
use lighting_core::{
    Memory, MemoryCandidate, MemoryId, MemoryKind, MemoryRelation, ReconciliationAction,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RememberRequest {
    pub content: String,
    pub scope: Option<String>,
    pub kind: Option<MemoryKind>,
    pub identity_key: Option<String>,
    pub confidence: Option<u8>,
    pub importance: Option<u8>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub episode_ids: Vec<String>,
    #[serde(default)]
    pub derived_from: Vec<MemoryId>,
    pub actor: Option<String>,
    pub sensitivity: Option<String>,
    #[serde(default)]
    pub historical: bool,
}

impl RememberRequest {
    pub fn into_candidate(self) -> Result<MemoryCandidate, &'static str> {
        if self.content.trim().is_empty() {
            return Err("memory content must not be blank");
        }
        Ok(MemoryCandidate {
            scope: self.scope,
            kind: self.kind.unwrap_or(MemoryKind::Fact),
            canonical_text: self.content,
            identity_key: self.identity_key,
            confidence: self.confidence.unwrap_or(70).min(100),
            importance: self.importance.unwrap_or(50).min(100),
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            source_ids: self.source_ids,
            episode_ids: self.episode_ids,
            derived_from: self.derived_from,
            actor: self.actor.unwrap_or_else(|| "lantern-api".to_owned()),
            sensitivity: self.sensitivity.unwrap_or_else(|| "normal".to_owned()),
            historical: self.historical,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct RememberResponse {
    pub action: ReconciliationAction,
    pub reason: String,
    pub affected_memory_ids: Vec<MemoryId>,
}

#[derive(Debug, Deserialize)]
pub struct MemorySearchQuery {
    pub q: String,
    pub scope: Option<String>,
    pub include_history: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct MemorySearchHit {
    pub memory: Memory,
    pub score: u32,
    pub why: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryContextRequest {
    pub query: String,
    pub scope: Option<String>,
    pub include_history: Option<bool>,
    pub budget: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct MemoryContextResponse {
    pub current_truth: Vec<MemorySearchHit>,
    pub history: Vec<MemorySearchHit>,
    pub unresolved: Vec<MemorySearchHit>,
}

#[derive(Debug, Serialize)]
pub struct MemoryHistoryResponse {
    pub identity_key: String,
    pub memories: Vec<Memory>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryRelationRequest {
    pub relation_id: Option<String>,
    pub from_memory_id: Option<MemoryId>,
    pub to_memory_id: Option<MemoryId>,
    pub relation_type: String,
    pub target_ref: String,
    pub source_id: Option<String>,
}

impl MemoryRelationRequest {
    pub fn into_domain(self) -> Result<MemoryRelation, &'static str> {
        if self.relation_type.trim().is_empty() {
            return Err("relation type must not be blank");
        }
        if self.target_ref.trim().is_empty() {
            return Err("relation target must not be blank");
        }
        let relation_id = self
            .relation_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(MemoryRelation::with_id(
            relation_id,
            self.from_memory_id,
            self.to_memory_id,
            self.relation_type,
            self.target_ref,
            self.source_id,
        ))
    }
}
