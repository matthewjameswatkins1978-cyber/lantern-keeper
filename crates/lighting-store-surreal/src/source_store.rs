use async_trait::async_trait;
use lighting_core::{
    Source, SourceContent, SourceFingerprint, SourceId, SourceKind, SourceRepository,
    SourceRepositoryError, SourceTitle, StoreSourceResult,
};
use surrealdb::types::ToSql;
use thiserror::Error;

use crate::connection::SurrealStore;

/// Errors raised by the SurrealDB-backed Source repository.

#[derive(Debug, Error)]
pub enum SurrealSourceRepositoryError {
    #[error("failed to apply source schema migration: {0}")]
    Migration(#[source] surrealdb::Error),
    #[error("failed to store source: {0}")]
    Store(#[source] surrealdb::Error),
    #[error("duplicate source fingerprint detected")]
    DuplicateFingerprint,
    #[error("failed to fetch source: {0}")]
    Fetch(#[source] surrealdb::Error),
    #[error("source record could not be decoded")]
    Decode,
}

impl From<SurrealSourceRepositoryError> for SourceRepositoryError {
    fn from(error: SurrealSourceRepositoryError) -> Self {
        match error {
            SurrealSourceRepositoryError::Migration(source)
            | SurrealSourceRepositoryError::Store(source)
            | SurrealSourceRepositoryError::Fetch(source) => {
                SourceRepositoryError::Operation(Box::new(source))
            }
            SurrealSourceRepositoryError::DuplicateFingerprint => {
                SourceRepositoryError::Operation(Box::new(error))
            }
            SurrealSourceRepositoryError::Decode => {
                SourceRepositoryError::Operation(Box::new(error))
            }
        }
    }
}

/// SurrealDB-backed implementation of [`SourceRepository`].
pub struct SurrealSourceRepository {
    store: SurrealStore,
}

impl SurrealSourceRepository {
    /// Creates a repository around an existing SurrealDB connection.
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    /// Applies the versioned schema migration needed by this repository.
    pub async fn migrate(&self) -> Result<(), SurrealSourceRepositoryError> {
        self.store
            .query(SCHEMA_MIGRATION_V1)
            .await
            .map(|_| ())
            .map_err(SurrealSourceRepositoryError::Migration)
    }

    /// Executes a raw SurrealQL query and returns the typed response.
    ///
    /// Intended for tests and diagnostic code that need to inspect the store
    /// directly without adding one-off operations to the repository trait.
    pub async fn raw_query(
        &self,
        query: &str,
    ) -> Result<surrealdb::IndexedResults, surrealdb::Error> {
        self.store.query(query).await
    }
}

#[async_trait]
impl SourceRepository for SurrealSourceRepository {
    async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError> {
        // 1. Look for an existing source with the same kind + title (same logical source).
        let existing = self
            .find_by_kind_and_title(source.kind(), source.title().as_str())
            .await
            .map_err(|e| SourceRepositoryError::Operation(Box::new(e)))?;

        if let Some(existing_source) = existing {
            // 2. Same logical source exists. Compare fingerprints.
            if existing_source.fingerprint() == source.fingerprint() {
                // Identical content → return the existing source.
                return Ok(StoreSourceResult::Duplicate {
                    existing_id: existing_source.id().clone(),
                    attempted: source,
                });
            } else {
                // Changed content → create new revision linked to the previous one.
                let id = source.id().as_str();
                let kind = kind_to_string(source.kind());
                let title = source.title().as_str();
                let content = source.content().as_str();
                let fingerprint = source.fingerprint().as_str();
                let created_at = source.created_at();
                let previous_id = existing_source.id().as_str();

                let result: Result<Vec<surrealdb::types::Object>, surrealdb::Error> = self
                    .store
                    .query(
                        r#"
                            CREATE source CONTENT {
                                id: $id,
                                kind: $kind,
                                title: $title,
                                content: $content,
                                fingerprint: $fingerprint,
                                created_at: $created_at,
                                previous_version_id: $previous_version_id
                            };
                        "#,
                    )
                    .bind(("id", id))
                    .bind(("kind", kind))
                    .bind(("title", title))
                    .bind(("content", content))
                    .bind(("fingerprint", fingerprint))
                    .bind(("created_at", created_at))
                    .bind(("previous_version_id", previous_id))
                    .await
                    .and_then(|mut response| response.take(0));

                match result {
                    Ok(_) => Ok(StoreSourceResult::Stored(source)),
                    Err(surrealdb_error) => Err(SourceRepositoryError::Operation(Box::new(
                        SurrealSourceRepositoryError::Store(surrealdb_error),
                    ))),
                }
            }
        } else {
            // 3. No existing logical source → fresh creation.
            let id = source.id().as_str();
            let kind = kind_to_string(source.kind());
            let title = source.title().as_str();
            let content = source.content().as_str();
            let fingerprint = source.fingerprint().as_str();
            let created_at = source.created_at();

            let result: Result<Vec<surrealdb::types::Object>, surrealdb::Error> = self
                .store
                .query(
                    r#"
                        CREATE source CONTENT {
                            id: $id,
                            kind: $kind,
                            title: $title,
                            content: $content,
                            fingerprint: $fingerprint,
                            created_at: $created_at
                        };
                    "#,
                )
                .bind(("id", id))
                .bind(("kind", kind))
                .bind(("title", title))
                .bind(("content", content))
                .bind(("fingerprint", fingerprint))
                .bind(("created_at", created_at))
                .await
                .and_then(|mut response| response.take(0));

            match result {
                Ok(_) => Ok(StoreSourceResult::Stored(source)),
                Err(error) if is_unique_conflict(&error) => {
                    let existing_id = self
                        .find_by_fingerprint(fingerprint)
                        .await
                        .map_err(|source| SourceRepositoryError::Operation(Box::new(source)))?;
                    Ok(StoreSourceResult::Duplicate {
                        existing_id: existing_id.ok_or_else(|| {
                            SourceRepositoryError::Operation(Box::new(
                                SurrealSourceRepositoryError::DuplicateFingerprint,
                            ))
                        })?,
                        attempted: source,
                    })
                }
                Err(source) => Err(SourceRepositoryError::Operation(Box::new(
                    SurrealSourceRepositoryError::Store(source),
                ))),
            }
        }
    }

    async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM source WHERE id = type::record('source', $id)")
            .bind(("id", id.as_str()))
            .await
            .map_err(|source| SourceRepositoryError::Operation(Box::new(source)))?
            .take(0)
            .map_err(|source| SourceRepositoryError::Operation(Box::new(source)))?;

        record.map(to_domain_source).transpose()
    }

    async fn get_current(
        &self,
        kind: SourceKind,
        title: &SourceTitle,
    ) -> Result<Option<Source>, SourceRepositoryError> {
        let kind_str = kind_to_string(kind);
        let title_str = title.as_str();

        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM source WHERE kind = $kind AND title = $title")
            .bind(("kind", kind_str.as_str()))
            .bind(("title", title_str))
            .await
            .map_err(|source| SourceRepositoryError::Operation(Box::new(source)))?
            .take(0)
            .map_err(|source| SourceRepositoryError::Operation(Box::new(source)))?;

        if records.is_empty() {
            return Ok(None);
        }

        let sources: Vec<Source> = records
            .into_iter()
            .map(to_domain_source)
            .collect::<Result<_, _>>()?;

        // Collect all IDs that are referenced as previous_version_id by another source.
        let referenced: std::collections::HashSet<SourceId> = sources
            .iter()
            .filter_map(|s| s.previous_version_id().cloned())
            .collect();

        // The current revision is the one NOT referenced by any other source.
        Ok(sources.into_iter().find(|s| !referenced.contains(s.id())))
    }

    async fn list_all_by_kind_and_title(
        &self,
        kind: SourceKind,
        title: &SourceTitle,
    ) -> Result<Vec<Source>, SourceRepositoryError> {
        self.list_by_kind_and_title(kind, title.as_str())
            .await
            .map_err(|e| SourceRepositoryError::Operation(Box::new(e)))
    }
}

impl SurrealSourceRepository {
    /// Lists all revisions for a logical (kind, title) Source, oldest first.
    async fn list_by_kind_and_title(
        &self,
        kind: SourceKind,
        title: &str,
    ) -> Result<Vec<Source>, SurrealSourceRepositoryError> {
        let kind_str = kind_to_string(kind);
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM source WHERE kind = $kind AND title = $title ORDER BY created_at ASC")
            .bind(("kind", kind_str.as_str()))
            .bind(("title", title))
            .await
            .map_err(SurrealSourceRepositoryError::Fetch)?
            .take(0)
            .map_err(SurrealSourceRepositoryError::Fetch)?;

        records
            .into_iter()
            .map(to_domain_source)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SurrealSourceRepositoryError::Decode)
    }

    async fn find_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<SourceId>, SurrealSourceRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM source WHERE fingerprint = $fingerprint LIMIT 1")
            .bind(("fingerprint", fingerprint))
            .await
            .map_err(SurrealSourceRepositoryError::Fetch)?
            .take(0)
            .map_err(SurrealSourceRepositoryError::Fetch)?;

        Ok(record
            .map(to_domain_source)
            .transpose()
            .map_err(|_| SurrealSourceRepositoryError::Decode)?
            .map(|s| s.id().clone()))
    }

    async fn find_by_kind_and_title(
        &self,
        kind: SourceKind,
        title: &str,
    ) -> Result<Option<Source>, SurrealSourceRepositoryError> {
        let kind_str = kind_to_string(kind);
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM source WHERE kind = $kind AND title = $title ORDER BY created_at DESC LIMIT 1")
            .bind(("kind", kind_str.as_str()))
            .bind(("title", title))
            .await
            .map_err(SurrealSourceRepositoryError::Fetch)?
            .take(0)
            .map_err(SurrealSourceRepositoryError::Fetch)?;

        record
            .map(to_domain_source)
            .transpose()
            .map_err(|_| SurrealSourceRepositoryError::Decode)
    }
}

fn kind_to_string(kind: SourceKind) -> String {
    match kind {
        SourceKind::PlainText => "plaintext".to_owned(),
        SourceKind::Markdown => "markdown".to_owned(),
    }
}

fn kind_from_string(value: &str) -> Option<SourceKind> {
    match value {
        "plaintext" => Some(SourceKind::PlainText),
        "markdown" => Some(SourceKind::Markdown),
        _ => None,
    }
}

fn is_unique_conflict(error: &surrealdb::Error) -> bool {
    if matches!(
        error.details(),
        surrealdb::types::ErrorDetails::AlreadyExists(_)
    ) {
        return true;
    }

    let message = error.message();
    message.contains("already contains") && message.contains("source_fingerprint")
}

fn record_id_key_to_string(key: &surrealdb::types::RecordIdKey) -> String {
    use surrealdb::types::RecordIdKey;
    match key {
        RecordIdKey::String(value) => value.clone(),
        RecordIdKey::Uuid(value) => value.to_string(),
        RecordIdKey::Number(value) => value.to_string(),
        other => other.to_sql(),
    }
}

fn to_domain_source(record: surrealdb::types::Object) -> Result<Source, SourceRepositoryError> {
    let obj = record.into_inner();

    let id = obj
        .get("id")
        .and_then(|value| value.clone().into_t::<surrealdb::types::RecordId>().ok())
        .map(|rid| record_id_key_to_string(&rid.key))
        .ok_or_else(|| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?;
    let kind = obj
        .get("kind")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .and_then(|kind| kind_from_string(&kind))
        .ok_or_else(|| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?;
    let title = obj
        .get("title")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .map(SourceTitle::new)
        .transpose()
        .map_err(|_| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?
        .ok_or_else(|| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?;
    let content = obj
        .get("content")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .map(SourceContent::new)
        .transpose()
        .map_err(|_| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?
        .ok_or_else(|| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?;
    let created_at = obj
        .get("created_at")
        .and_then(|value| value.clone().into_t::<surrealdb::types::Datetime>().ok())
        .map(|dt| dt.into())
        .ok_or_else(|| {
            SourceRepositoryError::Operation(Box::new(SurrealSourceRepositoryError::Decode))
        })?;

    let previous_version_id = obj.get("previous_version_id").and_then(|value| {
        // It can be either a string directly, or a record ID.
        if let Ok(s) = value.clone().into_t::<String>() {
            if s.is_empty() {
                return None;
            }
            Some(SourceId::from_record_id(s))
        } else if let Ok(rid) = value.clone().into_t::<surrealdb::types::RecordId>() {
            Some(SourceId::from_record_id(record_id_key_to_string(&rid.key)))
        } else {
            None
        }
    });

    let fingerprint = SourceFingerprint::for_content(&content);

    Ok(Source::reconstitute(
        SourceId::from_record_id(id),
        kind,
        title,
        content,
        fingerprint,
        created_at,
        previous_version_id,
    ))
}

const SCHEMA_MIGRATION_V1: &str = r#"
DEFINE TABLE IF NOT EXISTS source SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS kind ON source TYPE string;
DEFINE FIELD IF NOT EXISTS title ON source TYPE string;
DEFINE FIELD IF NOT EXISTS content ON source TYPE string;
DEFINE FIELD IF NOT EXISTS fingerprint ON source TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON source TYPE datetime;
DEFINE FIELD IF NOT EXISTS previous_version_id ON source TYPE option<string>;
DEFINE INDEX IF NOT EXISTS source_fingerprint ON source FIELDS fingerprint UNIQUE;

DEFINE TABLE IF NOT EXISTS __lighting_schema SCHEMALESS;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 1,
    updated_at: time::now()
};
"#;
