//! Engine-independent export for Lantern Keeper data.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use surrealdb::types::SurrealValue as _;
use surrealdb::types::{Array, Datetime, Object, Value as SurrealValue};
use thiserror::Error;

use crate::connection::SurrealStore;

const EXPORT_FORMAT: &str = "lantern-keeper-export-v1";

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("export filesystem operation failed: {0}")]
    Filesystem(#[source] std::io::Error),
    #[error("export manifest/record encoding failed: {0}")]
    Json(#[source] serde_json::Error),
    #[error("restore manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("restore query failed for {table}: {source}")]
    RestoreQuery {
        table: String,
        #[source]
        source: Box<surrealdb::Error>,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportSummary {
    pub format: &'static str,
    pub directory: PathBuf,
    pub tables: Vec<String>,
    pub record_count: usize,
    pub record_counts: BTreeMap<String, usize>,
    pub export_hash: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestoreSummary {
    pub format: String,
    pub directory: PathBuf,
    pub tables: Vec<String>,
    pub record_count: usize,
}

impl SurrealStore {
    /// Export the current store as newline-delimited JSON grouped by stable
    /// conceptual domains. Surreal-specific values are converted to JSON so
    /// the result remains readable without the original storage engine.
    pub async fn export_to(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<ExportSummary, ExportError> {
        let directory = directory.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(ExportError::Filesystem)?;

        let groups: [(&str, &[&str]); 4] = [
            (
                "ledger.ndjson",
                &[
                    "source",
                    "episode",
                    "marker",
                    "ledger_event",
                    "authority_grant",
                    "authority_revocation",
                    "receipt",
                ],
            ),
            ("projects.ndjson", &["project"]),
            (
                "relations.ndjson",
                &["episode_project_relation", "episode_marker_relation"],
            ),
            (
                "epistemic.ndjson",
                &[
                    "epistemic_claim",
                    "belief",
                    "memory_item",
                    "memory_relation",
                    "trace",
                    "proposal",
                    "predicate_definition",
                    "dimension_definition",
                    "belief_revision",
                    "context_pack",
                ],
            ),
        ];

        let mut tables = Vec::new();
        let mut record_count = 0;
        let mut record_counts = BTreeMap::new();
        let mut export_hasher = Sha256::new();

        for (file_name, table_names) in groups {
            let mut lines = Vec::new();
            for table in table_names {
                let records = select_table(self, table).await?;

                tables.push((*table).to_owned());
                record_count += records.len();
                record_counts.insert((*table).to_owned(), records.len());
                for record in records {
                    let mut line = Map::new();
                    line.insert("table".to_owned(), Value::String((*table).to_owned()));
                    line.insert("record".to_owned(), object_to_json(record));
                    lines.push(
                        serde_json::to_string(&Value::Object(line)).map_err(ExportError::Json)?,
                    );
                }
            }

            let contents = if lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", lines.join("\n"))
            };
            export_hasher.update(contents.as_bytes());
            tokio::fs::write(directory.join(file_name), contents)
                .await
                .map_err(ExportError::Filesystem)?;
        }

        let memory_records = select_table(self, "memory").await?;
        tables.push("memory".to_owned());
        record_count += memory_records.len();
        record_counts.insert("memory".to_owned(), memory_records.len());
        let memory_lines = memory_records
            .into_iter()
            .map(|record| {
                let mut line = Map::new();
                line.insert("table".to_owned(), Value::String("memory".to_owned()));
                line.insert("record".to_owned(), object_to_json(record));
                serde_json::to_string(&Value::Object(line)).map_err(ExportError::Json)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let memory_contents = if memory_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", memory_lines.join("\n"))
        };
        export_hasher.update(memory_contents.as_bytes());
        tokio::fs::write(directory.join("memories.ndjson"), memory_contents)
            .await
            .map_err(ExportError::Filesystem)?;
        let export_hash = format!("{:x}", export_hasher.finalize());
        let schema_version = self
            .schema_version()
            .await
            .map_err(|error| ExportError::InvalidManifest(error.to_string()))?;

        let manifest = serde_json::json!({
            "format": EXPORT_FORMAT,
            "generated_at": Utc::now(),
            "source_of_truth": "Lantern Keeper SurrealDB logical records",
            "tables": tables,
            "record_count": record_count,
            "record_counts": record_counts,
            "schema_version": schema_version,
            "export_hash": export_hash,
            "files": [
                "manifest.json",
                "ledger.ndjson",
                "memories.ndjson",
                "relations.ndjson",
                "projects.ndjson",
                "epistemic.ndjson"
            ]
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(ExportError::Json)?;
        tokio::fs::write(directory.join("manifest.json"), manifest_bytes)
            .await
            .map_err(ExportError::Filesystem)?;

        Ok(ExportSummary {
            format: EXPORT_FORMAT,
            directory,
            tables,
            record_count,
            record_counts,
            export_hash,
        })
    }

    /// Restore a logical export into a schema-initialised store.
    ///
    /// Records are UPSERTed by their exported Surreal record ID, so replaying
    /// the same directory is idempotent. The caller owns the choice of target
    /// store; normal restore drills must use a fresh isolated database.
    pub async fn restore_from(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<RestoreSummary, ExportError> {
        let directory = directory.as_ref().to_path_buf();
        let manifest_path = directory.join("manifest.json");
        let manifest_text = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(ExportError::Filesystem)?;
        let manifest: Value = serde_json::from_str(&manifest_text).map_err(ExportError::Json)?;
        if manifest.get("format").and_then(Value::as_str) != Some(EXPORT_FORMAT) {
            return Err(ExportError::InvalidManifest(format!(
                "expected format {EXPORT_FORMAT}"
            )));
        }
        let files = manifest
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| ExportError::InvalidManifest("files must be an array".to_owned()))?;
        let mut tables = Vec::new();
        let mut record_count = 0;
        for file in files {
            let file_name = file
                .as_str()
                .ok_or_else(|| ExportError::InvalidManifest("file name must be text".to_owned()))?;
            if file_name == "manifest.json" || !file_name.ends_with(".ndjson") {
                continue;
            }
            let path = directory.join(file_name);
            let contents = tokio::fs::read_to_string(&path)
                .await
                .map_err(ExportError::Filesystem)?;
            for (line_number, line) in contents.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let envelope: Value = serde_json::from_str(line).map_err(|error| {
                    ExportError::InvalidManifest(format!(
                        "{} line {} is not valid JSON: {error}",
                        path.display(),
                        line_number + 1
                    ))
                })?;
                let table = envelope
                    .get("table")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ExportError::InvalidManifest("record has no table".to_owned())
                    })?;
                if !is_export_table(table) {
                    return Err(ExportError::InvalidManifest(format!(
                        "table {table:?} is not restorable"
                    )));
                }
                let record = envelope.get("record").ok_or_else(|| {
                    ExportError::InvalidManifest("record envelope has no record".to_owned())
                })?;
                let record_id = record.get("id").and_then(Value::as_str).ok_or_else(|| {
                    ExportError::InvalidManifest(format!("{table} record has no text id"))
                })?;
                let record_key = restore_key(table, record_id)?;
                let target = format!("{table}:`{record_key}`");
                let existing: Vec<surrealdb::types::Object> = self
                    .query(format!("SELECT * FROM {target};"))
                    .await
                    .map_err(|source| ExportError::RestoreQuery {
                        table: table.to_owned(),
                        source: Box::new(source),
                    })?
                    .take(0)
                    .map_err(|source| ExportError::RestoreQuery {
                        table: table.to_owned(),
                        source: Box::new(source),
                    })?;
                if existing.is_empty() {
                    let mut restorable_record = record.clone();
                    restorable_record
                        .get_mut("id")
                        .expect("record id was checked above")
                        .clone_from(&Value::String(record_key));
                    let mut response = self
                        .query(format!("CREATE {table} CONTENT $record;"))
                        .bind(("record", json_to_surreal(&restorable_record, None)))
                        .await
                        .map_err(|source| ExportError::RestoreQuery {
                            table: table.to_owned(),
                            source: Box::new(source),
                        })?;
                    let _: Vec<surrealdb::types::Object> =
                        response
                            .take(0)
                            .map_err(|source| ExportError::RestoreQuery {
                                table: table.to_owned(),
                                source: Box::new(source),
                            })?;
                }
                if !tables.iter().any(|existing| existing == table) {
                    tables.push(table.to_owned());
                }
                record_count += 1;
            }
        }
        Ok(RestoreSummary {
            format: EXPORT_FORMAT.to_owned(),
            directory,
            tables,
            record_count,
        })
    }
}

fn json_to_surreal(value: &Value, key: Option<&str>) -> SurrealValue {
    match value {
        Value::Object(object) => SurrealValue::Object(Object::from_iter(
            object
                .iter()
                .map(|(name, value)| (name.clone(), json_to_surreal(value, Some(name)))),
        )),
        Value::Array(values) => SurrealValue::Array(Array::from(
            values
                .iter()
                .map(|value| json_to_surreal(value, None))
                .collect::<Vec<_>>(),
        )),
        Value::String(text) if key.is_some_and(is_datetime_field) => {
            match text
                .parse::<chrono::DateTime<chrono::FixedOffset>>()
                .ok()
                .map(|value| value.with_timezone(&Utc))
            {
                Some(value) => SurrealValue::Datetime(Datetime::from(value)),
                None => value.clone().into_value(),
            }
        }
        _ => value.clone().into_value(),
    }
}

fn is_datetime_field(field: &str) -> bool {
    matches!(
        field,
        "created_at"
            | "issued_at"
            | "expires_at"
            | "revoked_at"
            | "requested_at"
            | "updated_at"
            | "generated_at"
            | "received_at"
            | "known_at"
            | "recorded_at"
            | "observed_at"
            | "valid_from"
            | "valid_to"
            | "valid_until"
            | "superseded_at"
            | "stale_since"
            | "decided_at"
    )
}

fn is_export_table(table: &str) -> bool {
    matches!(
        table,
        "source"
            | "episode"
            | "marker"
            | "ledger_event"
            | "authority_grant"
            | "authority_revocation"
            | "receipt"
            | "project"
            | "episode_project_relation"
            | "episode_marker_relation"
            | "epistemic_claim"
            | "belief"
            | "belief_revision"
            | "memory_item"
            | "memory_relation"
            | "trace"
            | "proposal"
            | "predicate_definition"
            | "dimension_definition"
            | "memory"
    )
}

fn restore_key(table: &str, record_id: &str) -> Result<String, ExportError> {
    let prefix = format!("{table}:");
    let suffix = record_id.strip_prefix(&prefix).ok_or_else(|| {
        ExportError::InvalidManifest(format!(
            "record ID {record_id:?} does not belong to {table}"
        ))
    })?;
    let suffix = suffix.strip_prefix('`').unwrap_or(suffix);
    let suffix = suffix.strip_suffix('`').unwrap_or(suffix);
    if suffix.is_empty()
        || !suffix.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
    {
        return Err(ExportError::InvalidManifest(format!(
            "record ID {record_id:?} contains an unsafe record key"
        )));
    }
    Ok(suffix.to_owned())
}

fn object_to_json(object: surrealdb::types::Object) -> Value {
    Value::Object(
        object
            .into_inner()
            .into_iter()
            .map(|(key, value)| (key, value.into_json_value()))
            .collect(),
    )
}

async fn select_table(
    store: &SurrealStore,
    table: &str,
) -> Result<Vec<surrealdb::types::Object>, ExportError> {
    let mut result = match store.query(format!("SELECT * FROM {table}")).await {
        Ok(result) => result,
        Err(error) if error.to_string().contains("does not exist") => return Ok(Vec::new()),
        Err(error) => return Err(ExportError::Query(error)),
    };
    match result.take(0) {
        Ok(records) => Ok(records),
        Err(error) if error.to_string().contains("does not exist") => Ok(Vec::new()),
        Err(error) => Err(ExportError::Query(error)),
    }
}
