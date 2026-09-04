//! SurrealDB repository for derived Living Memory records.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    Memory, MemoryId, MemoryKind, MemoryRepository, MemoryRepositoryError, MemorySearchQuery,
    MemoryStatus, ProjectId,
};
use surrealdb::types::{Datetime, Object, RecordId, RecordIdKey, ToSql};
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealMemoryError {
    #[error("failed to execute query: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("memory record could not be decoded")]
    Decode,
    #[error("memory was not found")]
    NotFound,
}

impl From<SurrealMemoryError> for MemoryRepositoryError {
    fn from(error: SurrealMemoryError) -> Self {
        match error {
            SurrealMemoryError::NotFound => MemoryRepositoryError::NotFound,
            SurrealMemoryError::Decode | SurrealMemoryError::Query(_) => {
                MemoryRepositoryError::Operation(Box::new(error))
            }
        }
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
}

#[async_trait]
impl MemoryRepository for SurrealMemoryRepository {
    async fn store(&self, memory: Memory) -> Result<Memory, MemoryRepositoryError> {
        let record: Option<Object> = self
            .store
            .query(
                r#"
                CREATE memory CONTENT {
                    id: $id,
                    content: $content,
                    kind: $kind,
                    status: $status,
                    project_id: $project_id,
                    confidence: $confidence,
                    importance: $importance,
                    recorded_at: $recorded_at,
                    known_at: $known_at,
                    observed_at: $observed_at,
                    valid_from: $valid_from,
                    valid_until: $valid_until,
                    superseded_at: $superseded_at,
                    derived_from: $derived_from,
                    updates: $updates,
                    extends: $extends,
                    supersedes: $supersedes,
                    contradicts: $contradicts,
                    supports: $supports,
                    agent: $agent
                } RETURN AFTER;
                "#,
            )
            .bind(("id", memory.id.as_str()))
            .bind(("content", memory.content.clone()))
            .bind(("kind", memory.kind.as_str()))
            .bind(("status", memory.status.as_str()))
            .bind((
                "project_id",
                memory.project_id.as_ref().map(ToString::to_string),
            ))
            .bind(("confidence", memory.confidence))
            .bind(("importance", memory.importance))
            .bind(("recorded_at", memory.recorded_at))
            .bind(("known_at", memory.known_at))
            .bind(("observed_at", memory.observed_at))
            .bind(("valid_from", memory.valid_from))
            .bind(("valid_until", memory.valid_until))
            .bind(("superseded_at", memory.superseded_at))
            .bind(("derived_from", memory.derived_from.clone()))
            .bind(("updates", memory.updates.clone()))
            .bind(("extends", memory.extends.clone()))
            .bind(("supersedes", memory.supersedes.clone()))
            .bind(("contradicts", memory.contradicts.clone()))
            .bind(("supports", memory.supports.clone()))
            .bind(("agent", memory.agent.clone()))
            .await
            .map_err(|e| MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e))))?
            .take(0)
            .map_err(|e| {
                MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e)))
            })?;

        if record.is_some() {
            Ok(memory)
        } else {
            Err(MemoryRepositoryError::Operation(Box::new(
                SurrealMemoryError::Decode,
            )))
        }
    }

    async fn get(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError> {
        let record: Option<Object> = self
            .store
            .query("SELECT * FROM memory WHERE id = type::record('memory', $id)")
            .bind(("id", id.as_str()))
            .await
            .map_err(|e| MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e))))?
            .take(0)
            .map_err(|e| {
                MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e)))
            })?;

        record.map(decode_memory).transpose().map_err(Into::into)
    }

    async fn search(
        &self,
        query: &MemorySearchQuery,
    ) -> Result<Vec<Memory>, MemoryRepositoryError> {
        let mut sql = String::from("SELECT * FROM memory WHERE 1 = 1");
        if !query.include_inactive && query.as_of.is_none() {
            sql.push_str(" AND status = 'active'");
        }
        if query.as_of.is_some() {
            sql.push_str(" AND valid_from <= $as_of AND (valid_until IS NONE OR valid_until > $as_of)");
        }
        if query.project_id.is_some() {
            sql.push_str(" AND project_id = $project_id");
        }
        if query.phrase.is_some() {
            sql.push_str(" AND content CONTAINS $phrase");
        }
        sql.push_str(" ORDER BY importance DESC, confidence DESC, known_at DESC, id ASC");

        let mut request = self.store.query(sql);
        if let Some(project_id) = &query.project_id {
            request = request.bind(("project_id", project_id.to_string()));
        }
        if let Some(phrase) = &query.phrase {
            request = request.bind(("phrase", phrase.clone()));
        }
        if let Some(as_of) = query.as_of {
            request = request.bind(("as_of", as_of));
        }

        let records: Vec<Object> = request
            .await
            .map_err(|e| MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e))))?
            .take(0)
            .map_err(|e| {
                MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e)))
            })?;

        records
            .into_iter()
            .map(decode_memory)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    async fn supersede(
        &self,
        id: &MemoryId,
        at: DateTime<Utc>,
    ) -> Result<Memory, MemoryRepositoryError> {
        let record: Option<Object> = self
            .store
            .query(
                "UPDATE memory SET status = 'superseded', valid_until = $at, superseded_at = $at WHERE id = type::record('memory', $id) RETURN AFTER",
            )
            .bind(("id", id.as_str()))
            .bind(("at", at))
            .await
            .map_err(|e| MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e))))?
            .take(0)
            .map_err(|e| MemoryRepositoryError::Operation(Box::new(SurrealMemoryError::Query(e))))?;

        record
            .ok_or(MemoryRepositoryError::NotFound)
            .and_then(|record| decode_memory(record).map_err(Into::into))
    }

    async fn lineage(
        &self,
        id: &MemoryId,
        max_depth: usize,
    ) -> Result<Vec<Memory>, MemoryRepositoryError> {
        let mut result = Vec::new();
        let mut frontier = vec![(id.clone(), 0usize)];
        let mut visited = std::collections::HashSet::new();

        while let Some((current, depth)) = frontier.pop() {
            if depth > max_depth || !visited.insert(current.to_string()) {
                continue;
            }
            let Some(memory) = self.get(&current).await? else {
                continue;
            };
            let references = memory
                .derived_from
                .iter()
                .chain(memory.updates.iter())
                .chain(memory.extends.iter())
                .chain(memory.supersedes.iter())
                .chain(memory.contradicts.iter())
                .chain(memory.supports.iter());
            for reference in references {
                if let Ok(memory_id) = MemoryId::new(reference.clone()) {
                    frontier.push((memory_id, depth + 1));
                }
            }
            result.push(memory);
        }

        Ok(result)
    }
}

fn decode_memory(record: Object) -> Result<Memory, SurrealMemoryError> {
    let fields = record.into_inner();
    let id = fields
        .get("id")
        .and_then(|value| value.clone().into_t::<RecordId>().ok())
        .map(|record| record_key_to_string(&record.key))
        .and_then(|value| MemoryId::new(value).ok())
        .ok_or(SurrealMemoryError::Decode)?;
    let content = required::<String>(&fields, "content")?;
    let kind = MemoryKind::parse(&required::<String>(&fields, "kind")?)
        .ok_or(SurrealMemoryError::Decode)?;
    let status = MemoryStatus::parse(&required::<String>(&fields, "status")?)
        .ok_or(SurrealMemoryError::Decode)?;
    let project_id = optional::<String>(&fields, "project_id")?
        .map(|value| ProjectId::new(value).map_err(|_| SurrealMemoryError::Decode))
        .transpose()?;

    Ok(Memory {
        id,
        content,
        kind,
        status,
        project_id,
        confidence: required::<f32>(&fields, "confidence")?,
        importance: required::<f32>(&fields, "importance")?,
        recorded_at: required_datetime(&fields, "recorded_at")?,
        known_at: required_datetime(&fields, "known_at")?,
        observed_at: optional_datetime(&fields, "observed_at")?,
        valid_from: required_datetime(&fields, "valid_from")?,
        valid_until: optional_datetime(&fields, "valid_until")?,
        superseded_at: optional_datetime(&fields, "superseded_at")?,
        derived_from: required::<Vec<String>>(&fields, "derived_from")?,
        updates: optional::<Vec<String>>(&fields, "updates")?.unwrap_or_default(),
        extends: optional::<Vec<String>>(&fields, "extends")?.unwrap_or_default(),
        supersedes: required::<Vec<String>>(&fields, "supersedes")?,
        contradicts: required::<Vec<String>>(&fields, "contradicts")?,
        supports: required::<Vec<String>>(&fields, "supports")?,
        agent: required::<String>(&fields, "agent")?,
    })
}

fn required<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<T, SurrealMemoryError> {
    fields
        .get(name)
        .and_then(|value| value.clone().into_t::<T>().ok())
        .ok_or(SurrealMemoryError::Decode)
}

fn optional<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<Option<T>, SurrealMemoryError> {
    Ok(fields
        .get(name)
        .and_then(|value| value.clone().into_t::<Option<T>>().ok())
        .flatten())
}

fn required_datetime(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<DateTime<Utc>, SurrealMemoryError> {
    required::<Datetime>(fields, name).map(Into::into)
}

fn optional_datetime(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<Option<DateTime<Utc>>, SurrealMemoryError> {
    Ok(optional::<Datetime>(fields, name)?.map(Into::into))
}

fn record_key_to_string(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(value) => value.clone(),
        RecordIdKey::Uuid(value) => value.to_string(),
        RecordIdKey::Number(value) => value.to_string(),
        other => other.to_sql(),
    }
}

const SCHEMA_MIGRATION_V4: &str = r#"
DEFINE TABLE IF NOT EXISTS memory SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS content ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS status ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS confidence ON memory TYPE float;
DEFINE FIELD IF NOT EXISTS importance ON memory TYPE float;
DEFINE FIELD IF NOT EXISTS recorded_at ON memory TYPE datetime;
DEFINE FIELD IF NOT EXISTS known_at ON memory TYPE datetime;
DEFINE FIELD IF NOT EXISTS observed_at ON memory TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS valid_from ON memory TYPE datetime;
DEFINE FIELD IF NOT EXISTS valid_until ON memory TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS superseded_at ON memory TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS derived_from ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS updates ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS extends ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS supersedes ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS contradicts ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS supports ON memory TYPE array;
DEFINE FIELD IF NOT EXISTS agent ON memory TYPE string;
DEFINE INDEX IF NOT EXISTS memory_project ON memory FIELDS project_id;
DEFINE INDEX IF NOT EXISTS memory_status ON memory FIELDS status;
DEFINE INDEX IF NOT EXISTS memory_kind ON memory FIELDS kind;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 5,
    updated_at: time::now()
};
"#;
