//! SurrealDB repository for durable Bridge Intent records.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    BridgeClaimResult, BridgeIntentRecord, BridgeIntentRepository, BridgeIntentRepositoryError,
};
use surrealdb::types::{Datetime, Object, RecordIdKey, ToSql};
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealBridgeIntentError {
    #[error("failed to execute query: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("bridge intent record could not be decoded: {0}")]
    Decode(String),
    #[error("bridge intent was not found: {0}")]
    NotFound(String),
}

impl From<SurrealBridgeIntentError> for BridgeIntentRepositoryError {
    fn from(error: SurrealBridgeIntentError) -> Self {
        match error {
            SurrealBridgeIntentError::NotFound(id) => BridgeIntentRepositoryError::NotFound(id),
            SurrealBridgeIntentError::Decode(_) | SurrealBridgeIntentError::Query(_) => {
                BridgeIntentRepositoryError::Operation(Box::new(error))
            }
        }
    }
}

#[derive(Clone)]
pub struct SurrealBridgeIntentRepository {
    store: SurrealStore,
}

impl SurrealBridgeIntentRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealBridgeIntentError> {
        self.store
            .query(SCHEMA_MIGRATION_BRIDGE_INTENT)
            .await
            .map(|_| ())
            .map_err(SurrealBridgeIntentError::Query)
    }
}

#[async_trait]
impl BridgeIntentRepository for SurrealBridgeIntentRepository {
    async fn claim(
        &self,
        intent_id: &str,
        intent_commit: &str,
        canonical_digest: &str,
        action: &str,
    ) -> Result<BridgeClaimResult, BridgeIntentRepositoryError> {
        // 1. Check if record already exists
        if let Some(existing) = self.get(intent_id).await? {
            if existing.canonical_digest != canonical_digest {
                return Ok(BridgeClaimResult::Conflict {
                    existing_digest: existing.canonical_digest,
                    incoming_digest: canonical_digest.to_owned(),
                });
            }
            return Ok(BridgeClaimResult::Existing { record: existing });
        }

        // 2. Attempt to create record with status CLAIMED
        let now = Utc::now();
        let res = self
            .store
            .query(
                r#"
                CREATE type::record('bridge_intent', $intent_id) CONTENT {
                    intent_id: $intent_id,
                    intent_commit: $intent_commit,
                    canonical_digest: $canonical_digest,
                    action: $action,
                    status: 'CLAIMED',
                    lantern_record_id: NONE,
                    result_digest: NONE,
                    error: NONE,
                    processed_at: $processed_at
                } RETURN AFTER;
                "#,
            )
            .bind(("intent_id", intent_id.to_owned()))
            .bind(("intent_commit", intent_commit.to_owned()))
            .bind(("canonical_digest", canonical_digest.to_owned()))
            .bind(("action", action.to_owned()))
            .bind(("processed_at", now))
            .await;

        match res {
            Ok(mut resp) => {
                let record: Option<Object> = resp
                    .take(0)
                    .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;
                if let Some(obj) = record {
                    let rec = decode_intent(obj)?;
                    Ok(BridgeClaimResult::Claimed { record: rec })
                } else if let Some(existing) = self.get(intent_id).await? {
                    if existing.canonical_digest != canonical_digest {
                        return Ok(BridgeClaimResult::Conflict {
                            existing_digest: existing.canonical_digest,
                            incoming_digest: canonical_digest.to_owned(),
                        });
                    }
                    Ok(BridgeClaimResult::Existing { record: existing })
                } else {
                    Err(BridgeIntentRepositoryError::Operation(
                        "Failed to claim bridge intent record".into(),
                    ))
                }
            }
            Err(_) => {
                if let Some(existing) = self.get(intent_id).await? {
                    if existing.canonical_digest != canonical_digest {
                        return Ok(BridgeClaimResult::Conflict {
                            existing_digest: existing.canonical_digest,
                            incoming_digest: canonical_digest.to_owned(),
                        });
                    }
                    Ok(BridgeClaimResult::Existing { record: existing })
                } else {
                    Err(BridgeIntentRepositoryError::Operation(
                        "Failed to claim bridge intent record due to error".into(),
                    ))
                }
            }
        }
    }

    async fn complete(
        &self,
        intent_id: &str,
        status: &str,
        lantern_record_id: Option<String>,
        result_digest: Option<String>,
        error: Option<String>,
    ) -> Result<BridgeIntentRecord, BridgeIntentRepositoryError> {
        let now = Utc::now();
        let mut response = self
            .store
            .query(
                r#"
                UPDATE type::record('bridge_intent', $intent_id) MERGE {
                    status: $status,
                    lantern_record_id: $lantern_record_id,
                    result_digest: $result_digest,
                    error: $error,
                    processed_at: $processed_at
                } RETURN AFTER;
                "#,
            )
            .bind(("intent_id", intent_id.to_owned()))
            .bind(("status", status.to_owned()))
            .bind(("lantern_record_id", lantern_record_id))
            .bind(("result_digest", result_digest))
            .bind(("error", error))
            .bind(("processed_at", now))
            .await
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        let record: Option<Object> = response
            .take(0)
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        match record {
            Some(obj) => decode_intent(obj).map_err(Into::into),
            None => Err(BridgeIntentRepositoryError::NotFound(intent_id.to_owned())),
        }
    }

    async fn get(
        &self,
        intent_id: &str,
    ) -> Result<Option<BridgeIntentRecord>, BridgeIntentRepositoryError> {
        let mut response = self
            .store
            .query("SELECT * FROM bridge_intent WHERE id = type::record('bridge_intent', $intent_id) LIMIT 1;")
            .bind(("intent_id", intent_id.to_owned()))
            .await
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        let record: Option<Object> = response
            .take(0)
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        record.map(decode_intent).transpose().map_err(Into::into)
    }

    async fn list_all(&self) -> Result<Vec<BridgeIntentRecord>, BridgeIntentRepositoryError> {
        let mut response = self
            .store
            .query("SELECT * FROM bridge_intent ORDER BY processed_at ASC;")
            .await
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        let records: Vec<Object> = response
            .take(0)
            .map_err(|e| BridgeIntentRepositoryError::Operation(Box::new(e)))?;

        records
            .into_iter()
            .map(decode_intent)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn decode_intent(record: Object) -> Result<BridgeIntentRecord, SurrealBridgeIntentError> {
    let fields = record.into_inner();
    let intent_id = required::<String>(&fields, "intent_id")?;
    let intent_commit = required::<String>(&fields, "intent_commit")?;
    let canonical_digest = required::<String>(&fields, "canonical_digest")?;
    let action = required::<String>(&fields, "action")?;
    let status = required::<String>(&fields, "status")?;
    let lantern_record_id = optional::<String>(&fields, "lantern_record_id")?;
    let result_digest = optional::<String>(&fields, "result_digest")?;
    let error = optional::<String>(&fields, "error")?;
    let processed_at = required_datetime(&fields, "processed_at")?;

    Ok(BridgeIntentRecord {
        intent_id,
        intent_commit,
        canonical_digest,
        action,
        status,
        lantern_record_id,
        result_digest,
        error,
        processed_at,
    })
}

fn required<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<T, SurrealBridgeIntentError> {
    fields
        .get(name)
        .and_then(|value| value.clone().into_t::<T>().ok())
        .ok_or_else(|| SurrealBridgeIntentError::Decode(format!("missing field: {name}")))
}

fn optional<T: surrealdb::types::SurrealValue + 'static>(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<Option<T>, SurrealBridgeIntentError> {
    Ok(fields
        .get(name)
        .and_then(|value| value.clone().into_t::<Option<T>>().ok())
        .flatten())
}

fn required_datetime(
    fields: &std::collections::BTreeMap<String, surrealdb::types::Value>,
    name: &str,
) -> Result<DateTime<Utc>, SurrealBridgeIntentError> {
    required::<Datetime>(fields, name).map(Into::into)
}

fn _record_key_to_string(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(value) => value.clone(),
        RecordIdKey::Uuid(value) => value.to_string(),
        RecordIdKey::Number(value) => value.to_string(),
        other => other.to_sql(),
    }
}

const SCHEMA_MIGRATION_BRIDGE_INTENT: &str = r#"
DEFINE TABLE IF NOT EXISTS bridge_intent SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS intent_id ON bridge_intent TYPE string;
DEFINE FIELD IF NOT EXISTS intent_commit ON bridge_intent TYPE string;
DEFINE FIELD IF NOT EXISTS canonical_digest ON bridge_intent TYPE string;
DEFINE FIELD IF NOT EXISTS action ON bridge_intent TYPE string;
DEFINE FIELD IF NOT EXISTS status ON bridge_intent TYPE string;
DEFINE FIELD IF NOT EXISTS lantern_record_id ON bridge_intent TYPE option<string>;
DEFINE FIELD IF NOT EXISTS result_digest ON bridge_intent TYPE option<string>;
DEFINE FIELD IF NOT EXISTS error ON bridge_intent TYPE option<string>;
DEFINE FIELD IF NOT EXISTS processed_at ON bridge_intent TYPE datetime;
DEFINE INDEX IF NOT EXISTS bridge_intent_id ON bridge_intent FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS bridge_intent_intent_id ON bridge_intent FIELDS intent_id UNIQUE;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 11,
    updated_at: time::now()
};
"#;
