//! Durable append-only source-ledger event repository.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    LedgerEvent, LedgerEventRepository, LedgerIngestResult, LedgerRepositoryError, LedgerRole,
};
use surrealdb::types::{Datetime, Object, RecordId, RecordIdKey};
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealLedgerError {
    #[error("ledger query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("ledger event could not be decoded")]
    Decode,
}

#[derive(Clone)]
pub struct SurrealLedgerRepository {
    store: SurrealStore,
}

impl SurrealLedgerRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealLedgerError> {
        self.store
            .query(SCHEMA_MIGRATION_V6)
            .await
            .map(|_| ())
            .map_err(SurrealLedgerError::Query)
    }
}

#[async_trait]
impl LedgerEventRepository for SurrealLedgerRepository {
    async fn ingest(
        &self,
        event: LedgerEvent,
    ) -> Result<LedgerIngestResult, LedgerRepositoryError> {
        event
            .validate()
            .map_err(|error| LedgerRepositoryError::Operation(Box::new(error)))?;

        let existing: Option<Object> = self
            .store
            .query("SELECT * FROM ledger_event WHERE idempotency_key = $key LIMIT 1")
            .bind(("key", event.idempotency_key.clone()))
            .await
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?;
        if let Some(record) = existing {
            return Ok(LedgerIngestResult::Duplicate(
                decode_event(record).map_err(operation)?,
            ));
        }

        let record: Option<Object> = self
            .store
            .query(
                r#"CREATE ledger_event CONTENT {
                    id: $event_id,
                    event_id: $event_id,
                    source: $source,
                    external_id: $external_id,
                    session_id: $session_id,
                    conversation_id: $conversation_id,
                    turn_id: $turn_id,
                    actor: $actor,
                    role: $role,
                    content: $content,
                    observed_at: $observed_at,
                    received_at: $received_at,
                    reply_to: $reply_to,
                    project_hint: $project_hint,
                    idempotency_key: $idempotency_key,
                    raw_payload: $raw_payload,
                    metadata: $metadata
                } RETURN AFTER;"#,
            )
            .bind(("event_id", event.event_id.clone()))
            .bind(("source", event.source.clone()))
            .bind(("external_id", event.external_id.clone()))
            .bind(("session_id", event.session_id.clone()))
            .bind(("conversation_id", event.conversation_id.clone()))
            .bind(("turn_id", event.turn_id.clone()))
            .bind(("actor", event.actor.clone()))
            .bind(("role", role_name(event.role)))
            .bind(("content", event.content.clone()))
            .bind(("observed_at", event.observed_at))
            .bind(("received_at", event.received_at))
            .bind(("reply_to", event.reply_to.clone()))
            .bind(("project_hint", event.project_hint.clone()))
            .bind(("idempotency_key", event.idempotency_key.clone()))
            .bind(("raw_payload", event.raw_payload.clone()))
            .bind((
                "metadata",
                serde_json::to_string(&event.metadata).map_err(operation)?,
            ))
            .await
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?;

        if record.is_some() {
            Ok(LedgerIngestResult::Stored(event))
        } else {
            Err(operation(SurrealLedgerError::Decode))
        }
    }

    async fn list(&self, source: Option<&str>) -> Result<Vec<LedgerEvent>, LedgerRepositoryError> {
        let sql = if source.is_some() {
            "SELECT * FROM ledger_event WHERE source = $source ORDER BY received_at ASC, event_id ASC"
        } else {
            "SELECT * FROM ledger_event ORDER BY received_at ASC, event_id ASC"
        };
        let mut request = self.store.query(sql);
        if let Some(source) = source {
            request = request.bind(("source", source.to_owned()));
        }
        let records: Vec<Object> = request
            .await
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealLedgerError::Query(error)))?;
        records
            .into_iter()
            .map(decode_event)
            .collect::<Result<Vec<_>, _>>()
            .map_err(operation)
    }
}

fn operation(error: impl std::error::Error + Send + Sync + 'static) -> LedgerRepositoryError {
    LedgerRepositoryError::Operation(Box::new(error))
}

fn role_name(role: LedgerRole) -> &'static str {
    match role {
        LedgerRole::User => "user",
        LedgerRole::Assistant => "assistant",
        LedgerRole::System => "system",
        LedgerRole::Tool => "tool",
        LedgerRole::Unknown => "unknown",
    }
}

fn parse_role(value: &str) -> Option<LedgerRole> {
    Some(match value {
        "user" => LedgerRole::User,
        "assistant" => LedgerRole::Assistant,
        "system" => LedgerRole::System,
        "tool" => LedgerRole::Tool,
        "unknown" => LedgerRole::Unknown,
        _ => return None,
    })
}

fn decode_event(record: Object) -> Result<LedgerEvent, SurrealLedgerError> {
    let fields = record.into_inner();
    let event_id = fields
        .get("event_id")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .or_else(|| {
            fields
                .get("id")
                .and_then(|value| value.clone().into_t::<RecordId>().ok())
                .map(|record| record_key_to_string(&record.key))
        })
        .ok_or(SurrealLedgerError::Decode)?;
    let role = parse_role(&required::<String>(&fields, "role")?).ok_or(SurrealLedgerError::Decode)?;
    let event = LedgerEvent {
        event_id,
        source: required(&fields, "source")?,
        external_id: optional(&fields, "external_id")?,
        session_id: optional(&fields, "session_id")?,
        conversation_id: optional(&fields, "conversation_id")?,
        turn_id: optional(&fields, "turn_id")?,
        actor: required(&fields, "actor")?,
        role,
        content: required(&fields, "content")?,
        observed_at: optional_datetime(&fields, "observed_at")?,
        received_at: required_datetime(&fields, "received_at")?,
        reply_to: optional(&fields, "reply_to")?,
        project_hint: optional(&fields, "project_hint")?,
        idempotency_key: required(&fields, "idempotency_key")?,
        raw_payload: optional(&fields, "raw_payload")?,
        metadata: serde_json::from_str(&required::<String>(&fields, "metadata")?)
            .map_err(|_| SurrealLedgerError::Decode)?,
    };
    event.validate().map_err(|_| SurrealLedgerError::Decode)?;
    Ok(event)
}

fn required<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<T, SurrealLedgerError> {
    fields
        .get(name)
        .and_then(|value| value.clone().into_t::<T>().ok())
        .ok_or(SurrealLedgerError::Decode)
}

fn optional<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<Option<T>, SurrealLedgerError> {
    Ok(fields
        .get(name)
        .and_then(|value| value.clone().into_t::<Option<T>>().ok())
        .flatten())
}

fn required_datetime(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<DateTime<Utc>, SurrealLedgerError> {
    required::<Datetime>(fields, name).map(Into::into)
}

fn optional_datetime(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<Option<DateTime<Utc>>, SurrealLedgerError> {
    Ok(optional::<Datetime>(fields, name)?.map(Into::into))
}

fn record_key_to_string(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(value) => value.clone(),
        RecordIdKey::Uuid(value) => value.to_string(),
        RecordIdKey::Number(value) => value.to_string(),
        other => format!("{other:?}"),
    }
}

const SCHEMA_MIGRATION_V6: &str = r#"
DEFINE TABLE IF NOT EXISTS ledger_event SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS event_id ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS source ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS external_id ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS session_id ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS conversation_id ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS turn_id ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS actor ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS role ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS content ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS observed_at ON ledger_event TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS received_at ON ledger_event TYPE datetime;
DEFINE FIELD IF NOT EXISTS reply_to ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_hint ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS idempotency_key ON ledger_event TYPE string;
DEFINE FIELD IF NOT EXISTS raw_payload ON ledger_event TYPE option<string>;
DEFINE FIELD IF NOT EXISTS metadata ON ledger_event TYPE string;
DEFINE INDEX IF NOT EXISTS ledger_event_idempotency ON ledger_event FIELDS idempotency_key UNIQUE;
DEFINE INDEX IF NOT EXISTS ledger_event_source_received ON ledger_event FIELDS source, received_at;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 6,
    updated_at: time::now()
};
"#;
