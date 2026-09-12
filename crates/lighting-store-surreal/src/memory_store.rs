//! SurrealDB persistence for canonical Lantern memories.
//!
//! The canonical domain object is stored as a versioned JSON payload alongside
//! indexed fields. Keeping the payload intact makes exports, migrations and
//! schema evolution auditable while SurrealDB still provides useful filtering.

use async_trait::async_trait;
use chrono::Utc;
use lighting_core::{
    reconcile_candidate, Memory, MemoryCandidate, MemoryId, MemoryRelation,
    MemoryRelationRepository, MemoryRepository, MemoryRepositoryError, MemoryState,
    ReconciliationAction, ReconciliationDecision,
};
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealMemoryError {
    #[error("memory query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("memory payload could not be decoded")]
    Decode,
    #[error("memory payload could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("memory ID not found: {0}")]
    Missing(String),
}

impl From<SurrealMemoryError> for MemoryRepositoryError {
    fn from(value: SurrealMemoryError) -> Self {
        MemoryRepositoryError::Operation(Box::new(value))
    }
}

#[derive(Clone)]
pub struct SurrealMemoryRepository {
    store: SurrealStore,
}

impl SurrealMemoryRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealMemoryError> {
        self.store
            .query(SCHEMA_MIGRATION_V4)
            .await
            .map(|_| ())
            .map_err(SurrealMemoryError::Query)
    }

    pub async fn raw_query(
        &self,
        query: &str,
    ) -> Result<surrealdb::IndexedResults, surrealdb::Error> {
        self.store.query(query).await
    }

    async fn list_records(&self) -> Result<Vec<Memory>, SurrealMemoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT payload FROM memory ORDER BY updated_at DESC")
            .await
            .map_err(SurrealMemoryError::Query)?
            .take(0)
            .map_err(SurrealMemoryError::Query)?;

        records
            .into_iter()
            .map(|record| {
                let payload = record
                    .get("payload")
                    .and_then(|value| value.clone().into_t::<String>().ok())
                    .ok_or(SurrealMemoryError::Decode)?;
                serde_json::from_str(&payload).map_err(|_| SurrealMemoryError::Decode)
            })
            .collect()
    }

    async fn insert(&self, memory: &Memory) -> Result<(), SurrealMemoryError> {
        let payload = serde_json::to_string(memory).map_err(SurrealMemoryError::Encode)?;
        self.store
            .query(
                "CREATE memory CONTENT { memory_id: $memory_id, scope: $scope, kind: $kind, state: $state, identity_key: $identity_key, payload: $payload, updated_at: $updated_at };",
            )
            .bind(("memory_id", memory.id.as_str()))
            .bind(("scope", memory.scope.clone()))
            .bind(("kind", format!("{:?}", memory.kind).to_lowercase()))
            .bind(("state", state_name(memory.state)))
            .bind(("identity_key", memory.identity_key.clone()))
            .bind(("payload", payload))
            .bind(("updated_at", memory.updated_at))
            .await
            .map(|_| ())
            .map_err(SurrealMemoryError::Query)
    }

    async fn update(&self, memory: &Memory) -> Result<(), SurrealMemoryError> {
        let payload = serde_json::to_string(memory).map_err(SurrealMemoryError::Encode)?;
        self.store
            .query(
                "UPDATE memory SET scope = $scope, kind = $kind, state = $state, identity_key = $identity_key, payload = $payload, updated_at = $updated_at WHERE memory_id = $memory_id;",
            )
            .bind(("memory_id", memory.id.as_str()))
            .bind(("scope", memory.scope.clone()))
            .bind(("kind", format!("{:?}", memory.kind).to_lowercase()))
            .bind(("state", state_name(memory.state)))
            .bind(("identity_key", memory.identity_key.clone()))
            .bind(("payload", payload))
            .bind(("updated_at", memory.updated_at))
            .await
            .map(|_| ())
            .map_err(SurrealMemoryError::Query)
    }

    async fn record_event(
        &self,
        action: ReconciliationAction,
        memory_ids: &[MemoryId],
    ) -> Result<(), SurrealMemoryError> {
        let ids: Vec<String> = memory_ids.iter().map(|id| id.as_str().to_owned()).collect();
        self.store
            .query("CREATE memory_event CONTENT { action: $action, memory_ids: $memory_ids, recorded_at: time::now() };")
            .bind(("action", format!("{:?}", action).to_uppercase()))
            .bind(("memory_ids", ids))
            .await
            .map(|_| ())
            .map_err(SurrealMemoryError::Query)
    }

    pub async fn export_json(&self) -> Result<Vec<Memory>, SurrealMemoryError> {
        self.list_records().await
    }
}

#[async_trait]
impl MemoryRepository for SurrealMemoryRepository {
    async fn list(
        &self,
        scope: Option<&str>,
        include_history: bool,
    ) -> Result<Vec<Memory>, MemoryRepositoryError> {
        let mut memories = self.list_records().await?;
        memories.retain(|memory| {
            scope.is_none_or(|wanted| memory.scope.as_deref() == Some(wanted))
                && (include_history || memory.state == MemoryState::Active)
        });
        Ok(memories)
    }

    async fn get(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError> {
        Ok(self
            .list_records()
            .await?
            .into_iter()
            .find(|memory| memory.id == *id))
    }

    async fn reconcile_and_apply(
        &self,
        candidate: MemoryCandidate,
    ) -> Result<ReconciliationDecision, MemoryRepositoryError> {
        let existing = self.list_records().await?;
        let decision = reconcile_candidate(candidate, &existing);
        let now = Utc::now();
        let mut affected = Vec::new();

        match decision.action {
            ReconciliationAction::Ignore => {}
            ReconciliationAction::Forget => {}
            ReconciliationAction::New | ReconciliationAction::Historical => {
                let mut memory = Memory::from_candidate(decision.candidate.clone(), now);
                if decision.action == ReconciliationAction::Historical {
                    memory.state = MemoryState::Archived;
                    memory.checksum = memory.compute_checksum();
                }
                affected.push(memory.id.clone());
                self.insert(&memory).await?;
            }
            ReconciliationAction::Reinforce => {
                if let Some(id) = decision.matched_memory_ids.first() {
                    let mut memory = existing
                        .iter()
                        .find(|memory| memory.id == *id)
                        .cloned()
                        .ok_or_else(|| SurrealMemoryError::Missing(id.as_str().to_owned()))?;
                    memory.reinforcement_count = memory.reinforcement_count.saturating_add(1);
                    memory.last_reinforced_at = Some(now);
                    memory.updated_at = now;
                    memory
                        .source_ids
                        .extend(decision.candidate.source_ids.clone());
                    memory.source_ids.sort();
                    memory.source_ids.dedup();
                    memory.revision = memory.revision.saturating_add(1);
                    memory.checksum = memory.compute_checksum();
                    affected.push(memory.id.clone());
                    self.update(&memory).await?;
                }
            }
            ReconciliationAction::Supersede => {
                let old_id = decision
                    .matched_memory_ids
                    .iter()
                    .find(|id| {
                        existing
                            .iter()
                            .any(|memory| memory.id == **id && memory.state == MemoryState::Active)
                    })
                    .cloned()
                    .ok_or_else(|| SurrealMemoryError::Missing("active match".to_owned()))?;
                let mut old = existing
                    .iter()
                    .find(|memory| memory.id == old_id)
                    .cloned()
                    .unwrap();
                let mut next = Memory::from_candidate(decision.candidate.clone(), now);
                next.supersedes = Some(old.id.clone());
                old.state = MemoryState::Superseded;
                old.valid_to = next.valid_from.or(Some(now));
                old.superseded_by = Some(next.id.clone());
                old.updated_at = now;
                old.revision = old.revision.saturating_add(1);
                old.checksum = old.compute_checksum();
                affected.extend([old.id.clone(), next.id.clone()]);
                self.update(&old).await?;
                self.insert(&next).await?;
            }
            ReconciliationAction::Conflict
            | ReconciliationAction::Unresolved
            | ReconciliationAction::Ask => {
                let mut memory = Memory::from_candidate(decision.candidate.clone(), now);
                memory.state = MemoryState::NeedsReview;
                memory.conflicts_with = decision.matched_memory_ids.clone();
                memory.checksum = memory.compute_checksum();
                affected.push(memory.id.clone());
                self.insert(&memory).await?;
            }
            ReconciliationAction::Correct => {
                let mut memory = Memory::from_candidate(decision.candidate.clone(), now);
                memory.supersedes = decision.matched_memory_ids.first().cloned();
                affected.push(memory.id.clone());
                self.insert(&memory).await?;
            }
        }

        self.record_event(decision.action, &affected).await?;
        Ok(ReconciliationDecision {
            matched_memory_ids: affected,
            ..decision
        })
    }

    async fn forget(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError> {
        let Some(mut memory) = self
            .list_records()
            .await?
            .into_iter()
            .find(|memory| memory.id == *id)
        else {
            return Ok(None);
        };
        if memory.state != MemoryState::Tombstoned {
            memory.state = MemoryState::Tombstoned;
            memory.updated_at = Utc::now();
            memory.revision = memory.revision.saturating_add(1);
            memory.checksum = memory.compute_checksum();
            self.update(&memory).await?;
            self.record_event(ReconciliationAction::Forget, std::slice::from_ref(id))
                .await?;
        }
        Ok(Some(memory))
    }
}

#[async_trait]
impl MemoryRelationRepository for SurrealMemoryRepository {
    async fn upsert_relation(
        &self,
        relation: MemoryRelation,
    ) -> Result<MemoryRelation, MemoryRepositoryError> {
        let relation_json = serde_json::to_string(&relation)
            .map_err(SurrealMemoryError::Encode)
            .map_err(MemoryRepositoryError::from)?;
        self.store
            .query(
                "UPSERT memory_relation SET relation_id = $relation_id, from_memory_id = $from_memory_id, to_memory_id = $to_memory_id, relation_type = $relation_type, target_ref = $target_ref, source_id = $source_id, payload = $payload WHERE relation_id = $relation_id;",
            )
            .bind(("relation_id", relation.relation_id.clone()))
            .bind((
                "from_memory_id",
                relation.from_memory_id.as_ref().map(MemoryId::as_str),
            ))
            .bind((
                "to_memory_id",
                relation.to_memory_id.as_ref().map(MemoryId::as_str),
            ))
            .bind(("relation_type", relation.relation_type.clone()))
            .bind(("target_ref", relation.target_ref.clone()))
            .bind(("source_id", relation.source_id.clone()))
            .bind(("payload", relation_json))
            .await
            .map_err(SurrealMemoryError::Query)
            .map_err(MemoryRepositoryError::from)?;
        Ok(relation)
    }

    async fn list_relations(
        &self,
        memory_id: Option<&MemoryId>,
    ) -> Result<Vec<MemoryRelation>, MemoryRepositoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT payload FROM memory_relation ORDER BY relation_id")
            .await
            .map_err(SurrealMemoryError::Query)
            .map_err(MemoryRepositoryError::from)?
            .take(0)
            .map_err(SurrealMemoryError::Query)
            .map_err(MemoryRepositoryError::from)?;
        let relations = records
            .into_iter()
            .filter_map(|record| {
                let payload = record
                    .get("payload")
                    .and_then(|value| value.clone().into_t::<String>().ok())?;
                serde_json::from_str::<MemoryRelation>(&payload).ok()
            })
            .filter(|relation| {
                memory_id.is_none_or(|id| {
                    relation.from_memory_id.as_ref() == Some(id)
                        || relation.to_memory_id.as_ref() == Some(id)
                })
            })
            .collect();
        Ok(relations)
    }
}

fn state_name(state: MemoryState) -> &'static str {
    match state {
        MemoryState::Active => "active",
        MemoryState::Superseded => "superseded",
        MemoryState::Archived => "archived",
        MemoryState::Tombstoned => "tombstoned",
        MemoryState::NeedsReview => "needs_review",
    }
}

const SCHEMA_MIGRATION_V4: &str = r#"
DEFINE TABLE IF NOT EXISTS memory SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS memory_id ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS scope ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS kind ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS state ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS identity_key ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS payload ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON memory TYPE datetime;
DEFINE INDEX IF NOT EXISTS memory_id_unique ON memory FIELDS memory_id UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_identity ON memory FIELDS identity_key;
DEFINE TABLE IF NOT EXISTS memory_event SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS action ON memory_event TYPE string;
DEFINE FIELD IF NOT EXISTS memory_ids ON memory_event TYPE array<string>;
DEFINE FIELD IF NOT EXISTS recorded_at ON memory_event TYPE datetime;
DEFINE TABLE IF NOT EXISTS memory_relation SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS relation_id ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS from_memory_id ON memory_relation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS to_memory_id ON memory_relation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS relation_type ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS target_ref ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS source_id ON memory_relation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS payload ON memory_relation TYPE string;
DEFINE INDEX IF NOT EXISTS memory_relation_id_unique ON memory_relation FIELDS relation_id UNIQUE;
UPSERT __lighting_schema:bootstrap CONTENT { project: "Lantern Keeper", service: "Lighting", schema_version: 4, updated_at: time::now() };
"#;
