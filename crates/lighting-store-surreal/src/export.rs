//! Engine-independent export for Lantern Keeper data.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Map, Value};
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
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportSummary {
    pub format: &'static str,
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

        let groups: [(&str, &[&str]); 3] = [
            ("ledger.ndjson", &["source", "episode", "marker"]),
            ("projects.ndjson", &["project"]),
            (
                "relations.ndjson",
                &["episode_project_relation", "episode_marker_relation"],
            ),
        ];

        let mut tables = Vec::new();
        let mut record_count = 0;

        for (file_name, table_names) in groups {
            let mut lines = Vec::new();
            for table in table_names {
                let records: Vec<surrealdb::types::Object> = self
                    .query(format!("SELECT * FROM {table}"))
                    .await
                    .map_err(ExportError::Query)?
                    .take(0)
                    .map_err(ExportError::Query)?;

                tables.push((*table).to_owned());
                record_count += records.len();
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
            tokio::fs::write(directory.join(file_name), contents)
                .await
                .map_err(ExportError::Filesystem)?;
        }

        let memory_records: Vec<surrealdb::types::Object> = self
            .query("SELECT * FROM memory")
            .await
            .map_err(ExportError::Query)?
            .take(0)
            .map_err(ExportError::Query)?;
        tables.push("memory".to_owned());
        record_count += memory_records.len();
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
        tokio::fs::write(directory.join("memories.ndjson"), memory_contents)
            .await
            .map_err(ExportError::Filesystem)?;

        let manifest = serde_json::json!({
            "format": EXPORT_FORMAT,
            "generated_at": Utc::now(),
            "source_of_truth": "Lantern Keeper SurrealDB logical records",
            "tables": tables,
            "record_count": record_count,
            "files": [
                "manifest.json",
                "ledger.ndjson",
                "memories.ndjson",
                "relations.ndjson",
                "projects.ndjson"
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
        })
    }
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
