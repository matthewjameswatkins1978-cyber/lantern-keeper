//! SurrealDB-backed memory-path repository.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    Episode, EpisodeId, EpisodeMarkerLink, EpisodeProjectLink, EpisodeTitle, Marker, MarkerId,
    MemoryPathRepository, MemoryPathRepositoryError, Project, ProjectId, ProjectLinkKind,
    ProjectName, ProjectStatus, SourceId, SourceRange, StoreMarkerResult,
};
use surrealdb::types::ToSql;
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealMemoryPathError {
    #[error("failed to execute query: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("record decoding failed")]
    Decode,
    #[error("referenced Source does not exist")]
    MissingSource,
}

impl From<SurrealMemoryPathError> for MemoryPathRepositoryError {
    fn from(e: SurrealMemoryPathError) -> Self {
        match e {
            SurrealMemoryPathError::MissingSource => MemoryPathRepositoryError::MissingSource,
            SurrealMemoryPathError::Decode => MemoryPathRepositoryError::Operation(Box::new(e)),
            SurrealMemoryPathError::Query(source) => {
                MemoryPathRepositoryError::Operation(Box::new(source))
            }
        }
    }
}

pub struct SurrealMemoryPathRepository {
    store: SurrealStore,
}

impl SurrealMemoryPathRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    /// Applies all schema migrations (V2 scaffolding + V3 native relations).
    pub async fn migrate(&self) -> Result<(), SurrealMemoryPathError> {
        self.store
            .query(SCHEMA_MIGRATION_V2)
            .await
            .map(|_| ())
            .map_err(SurrealMemoryPathError::Query)?;
        self.store
            .query(SCHEMA_MIGRATION_V3)
            .await
            .map(|_| ())
            .map_err(SurrealMemoryPathError::Query)
    }

    /// Raw query access for tests.
    pub async fn raw_query(
        &self,
        query: &str,
    ) -> Result<surrealdb::IndexedResults, surrealdb::Error> {
        self.store.query(query).await
    }
}

#[async_trait]
impl MemoryPathRepository for SurrealMemoryPathRepository {
    async fn create_project(&self, project: Project) -> Result<Project, MemoryPathRepositoryError> {
        let result: Option<surrealdb::types::Object> = self
            .store
            .query(
                r#"
                CREATE project CONTENT {
                    id: $id,
                    name: $name,
                    status: $status,
                    created_at: $created_at
                };
                "#,
            )
            .bind(("id", project.id().as_str()))
            .bind(("name", project.name().as_str()))
            .bind(("status", status_to_str(project.status())))
            .bind(("created_at", project.created_at()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        match result {
            Some(_) => Ok(project),
            None => Err(MemoryPathRepositoryError::Decode),
        }
    }

    async fn get_project(
        &self,
        id: &ProjectId,
    ) -> Result<Option<Project>, MemoryPathRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM project WHERE id = type::record('project', $id)")
            .bind(("id", id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        record.map(to_domain_project).transpose()
    }

    async fn create_episode(&self, episode: Episode) -> Result<Episode, MemoryPathRepositoryError> {
        let source_id = episode.source_range().source_id().as_str();

        // Verify Source exists
        let source_exists: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT id FROM source WHERE id = type::record('source', $sid) LIMIT 1")
            .bind(("sid", source_id))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        if source_exists.is_none() {
            return Err(MemoryPathRepositoryError::MissingSource);
        }

        let result: Option<surrealdb::types::Object> = self
            .store
            .query(
                r#"
                CREATE episode CONTENT {
                    id: $id,
                    title: $title,
                    source_id: $source_id,
                    start_byte: $start_byte,
                    end_byte: $end_byte,
                    created_at: $created_at
                };
                "#,
            )
            .bind(("id", episode.id().as_str()))
            .bind(("title", episode.title().as_str()))
            .bind(("source_id", source_id))
            .bind(("start_byte", episode.source_range().start_byte() as i64))
            .bind(("end_byte", episode.source_range().end_byte() as i64))
            .bind(("created_at", episode.created_at()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        match result {
            Some(_) => Ok(episode),
            None => Err(MemoryPathRepositoryError::Decode),
        }
    }

    async fn get_episode(
        &self,
        id: &EpisodeId,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM episode WHERE id = type::record('episode', $id)")
            .bind(("id", id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        record.map(to_domain_episode).transpose()
    }

    async fn find_episode_by_source_range(
        &self,
        source_id: &SourceId,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query(
                "SELECT * FROM episode WHERE source_id = $source_id AND start_byte = $start_byte AND end_byte = $end_byte ORDER BY created_at ASC LIMIT 1",
            )
            .bind(("source_id", source_id.as_str()))
            .bind(("start_byte", start_byte as i64))
            .bind(("end_byte", end_byte as i64))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        record.map(to_domain_episode).transpose()
    }

    async fn create_marker(
        &self,
        marker: Marker,
    ) -> Result<StoreMarkerResult, MemoryPathRepositoryError> {
        // Check if lookup key already exists
        let existing: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM marker WHERE lookup_key = $lk LIMIT 1")
            .bind(("lk", marker.lookup_key()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        if let Some(record) = existing {
            let existing_marker = to_domain_marker(record)?;
            return Ok(StoreMarkerResult::Existing(existing_marker));
        }

        let result: Option<surrealdb::types::Object> = self
            .store
            .query(
                r#"
                CREATE marker CONTENT {
                    id: $id,
                    display_text: $display_text,
                    lookup_key: $lookup_key,
                    created_at: $created_at
                };
                "#,
            )
            .bind(("id", marker.id().as_str()))
            .bind(("display_text", marker.display_text()))
            .bind(("lookup_key", marker.lookup_key()))
            .bind(("created_at", marker.created_at()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        match result {
            Some(_) => Ok(StoreMarkerResult::Created(marker)),
            None => Err(MemoryPathRepositoryError::Decode),
        }
    }

    async fn get_marker(&self, id: &MarkerId) -> Result<Option<Marker>, MemoryPathRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM marker WHERE id = type::record('marker', $id)")
            .bind(("id", id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        record.map(to_domain_marker).transpose()
    }

    async fn find_marker_by_lookup(
        &self,
        lookup_key: &str,
    ) -> Result<Option<Marker>, MemoryPathRepositoryError> {
        let record: Option<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM marker WHERE lookup_key = $lk LIMIT 1")
            .bind(("lk", lookup_key))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        record.map(to_domain_marker).transpose()
    }

    async fn link_episode_project(
        &self,
        link: EpisodeProjectLink,
    ) -> Result<(), MemoryPathRepositoryError> {
        let kind = link_kind_to_str(link.kind());
        self.store
            .query(
                "\
                LET $from = type::record('episode', $eid); \
                LET $to = type::record('project', $pid); \
                RELATE $from->episode_project_relation->$to SET kind = $kind; \
                ",
            )
            .bind(("eid", link.episode_id().as_str()))
            .bind(("pid", link.project_id().as_str()))
            .bind(("kind", kind))
            .await
            .map(|_| ())
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))
    }

    async fn link_episode_marker(
        &self,
        link: EpisodeMarkerLink,
    ) -> Result<(), MemoryPathRepositoryError> {
        self.store
            .query(
                "\
                LET $from = type::record('episode', $eid); \
                LET $to = type::record('marker', $mid); \
                RELATE $from->episode_marker_relation->$to; \
                ",
            )
            .bind(("eid", link.episode_id().as_str()))
            .bind(("mid", link.marker_id().as_str()))
            .await
            .map(|_| ())
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))
    }

    async fn list_episode_project_links(
        &self,
        episode_id: &EpisodeId,
    ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query(
                "SELECT * FROM episode_project_relation WHERE in = type::record('episode', $eid)",
            )
            .bind(("eid", episode_id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        records
            .into_iter()
            .map(to_domain_episode_project_link)
            .collect()
    }

    async fn list_episode_marker_links(
        &self,
        episode_id: &EpisodeId,
    ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM episode_marker_relation WHERE in = type::record('episode', $eid)")
            .bind(("eid", episode_id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        records
            .into_iter()
            .map(to_domain_episode_marker_link)
            .collect()
    }

    async fn list_marker_episode_links(
        &self,
        marker_id: &MarkerId,
    ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError> {
        // V3 relation: RELATE episode->episode_marker_relation->marker
        // so 'in' = episode, 'out' = marker.
        // Query episodes linked to this marker: WHERE out = marker
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query(
                "\
                SELECT * FROM episode_marker_relation \
                WHERE out = type::record('marker', $mid) \
                LIMIT 50;\
                ",
            )
            .bind(("mid", marker_id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        records
            .into_iter()
            .map(to_domain_episode_marker_link)
            .collect()
    }

    async fn list_project_episode_links(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError> {
        // V3 relation: RELATE episode->episode_project_relation->project
        // so 'in' = episode, 'out' = project.
        // Query episodes linked to this project: WHERE out = project
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query(
                "\
                SELECT * FROM episode_project_relation \
                WHERE out = type::record('project', $pid) \
                LIMIT 50;\
                ",
            )
            .bind(("pid", project_id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        records
            .into_iter()
            .map(to_domain_episode_project_link)
            .collect()
    }

    async fn find_project_episode_by_source_range(
        &self,
        project_id: &ProjectId,
        source_id: &SourceId,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query(
                "SELECT * FROM episode_project_relation \
                 WHERE out = type::record('project', $pid) \
                 LIMIT 50;",
            )
            .bind(("pid", project_id.as_str()))
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        for record in records {
            let obj = record.into_inner();
            let in_rec = match obj
                .get("in")
                .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
            {
                Some(r) => r,
                None => continue,
            };

            let episode_result: Option<surrealdb::types::Object> = self
                .store
                .query("SELECT * FROM episode WHERE id = $eid")
                .bind(("eid", in_rec.clone()))
                .await
                .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
                .take(0)
                .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

            let episode = match episode_result {
                Some(obj) => match to_domain_episode(obj) {
                    Ok(ep) => ep,
                    Err(_) => continue,
                },
                None => continue,
            };

            if episode.source_range().source_id().as_str() == source_id.as_str()
                && episode.source_range().start_byte() == start_byte
                && episode.source_range().end_byte() == end_byte
            {
                return Ok(Some(episode));
            }
        }

        Ok(None)
    }

    async fn list_all_projects(&self) -> Result<Vec<Project>, MemoryPathRepositoryError> {
        let records: Vec<surrealdb::types::Object> = self
            .store
            .query("SELECT * FROM project ORDER BY created_at ASC, id ASC")
            .await
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?
            .take(0)
            .map_err(|e| MemoryPathRepositoryError::Operation(Box::new(e)))?;

        records.into_iter().map(to_domain_project).collect()
    }
}

// ---------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------

fn record_id_key(id: &surrealdb::types::RecordId) -> String {
    use surrealdb::types::RecordIdKey;
    match &id.key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Uuid(u) => u.to_string(),
        RecordIdKey::Number(n) => n.to_string(),
        other => other.to_sql(),
    }
}

fn to_domain_project(
    record: surrealdb::types::Object,
) -> Result<Project, MemoryPathRepositoryError> {
    let obj = record.into_inner();
    let id = obj
        .get("id")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let name = obj
        .get("name")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let status_str = obj
        .get("status")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let created_at = obj
        .get("created_at")
        .and_then(|v| v.clone().into_t::<surrealdb::types::Datetime>().ok())
        .map(Into::<DateTime<Utc>>::into)
        .ok_or(MemoryPathRepositoryError::Decode)?;

    // Reconstitute — we can't use Project::new because it generates a new ID
    // and timestamp. Instead we destructure what we have. But Project fields are private.
    // We need to use a different approach. Let's reconstruct via reconstitute.
    Ok(reconstitute_project(
        ProjectId::new(record_id_key(&id)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        ProjectName::new(name).map_err(|_| MemoryPathRepositoryError::Decode)?,
        match status_str.as_str() {
            "active" => ProjectStatus::Active,
            "paused" => ProjectStatus::Paused,
            "archived" => ProjectStatus::Archived,
            _ => return Err(MemoryPathRepositoryError::Decode),
        },
        created_at,
    ))
}

fn to_domain_episode(
    record: surrealdb::types::Object,
) -> Result<Episode, MemoryPathRepositoryError> {
    let obj = record.into_inner();
    let id = obj
        .get("id")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let title = obj
        .get("title")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let source_id = obj
        .get("source_id")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let start_byte = obj
        .get("start_byte")
        .and_then(|v| v.clone().into_t::<i64>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)? as usize;
    let end_byte = obj
        .get("end_byte")
        .and_then(|v| v.clone().into_t::<i64>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)? as usize;
    let created_at = obj
        .get("created_at")
        .and_then(|v| v.clone().into_t::<surrealdb::types::Datetime>().ok())
        .map(Into::<DateTime<Utc>>::into)
        .ok_or(MemoryPathRepositoryError::Decode)?;

    Ok(reconstitute_episode(
        EpisodeId::new(record_id_key(&id)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        EpisodeTitle::new(title).map_err(|_| MemoryPathRepositoryError::Decode)?,
        // We cannot reconstruct SourceRange without content, so we need
        // a lightweight reconstitution that preserves the byte range
        // but skips content validation. The range was validated when
        // the Episode was created. This matches the pattern in source_store.rs
        // where Source is reconstituted from raw fields.
        SourceRange::reconstitute(SourceId::from_record_id(source_id), start_byte, end_byte),
        created_at,
    ))
}

fn to_domain_marker(record: surrealdb::types::Object) -> Result<Marker, MemoryPathRepositoryError> {
    let obj = record.into_inner();
    let id = obj
        .get("id")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let display_text = obj
        .get("display_text")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let lookup_key = obj
        .get("lookup_key")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let created_at = obj
        .get("created_at")
        .and_then(|v| v.clone().into_t::<surrealdb::types::Datetime>().ok())
        .map(Into::<DateTime<Utc>>::into)
        .ok_or(MemoryPathRepositoryError::Decode)?;

    Ok(reconstitute_marker(
        MarkerId::new(record_id_key(&id)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        display_text,
        lookup_key,
        created_at,
    ))
}

fn to_domain_episode_project_link(
    record: surrealdb::types::Object,
) -> Result<EpisodeProjectLink, MemoryPathRepositoryError> {
    let obj = record.into_inner();
    let in_rec = obj
        .get("in")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let out_rec = obj
        .get("out")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let kind_str = obj
        .get("kind")
        .and_then(|v| v.clone().into_t::<String>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;

    Ok(EpisodeProjectLink::new(
        EpisodeId::new(record_id_key(&in_rec)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        ProjectId::new(record_id_key(&out_rec)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        match kind_str.as_str() {
            "primary" => ProjectLinkKind::Primary,
            "secondary" => ProjectLinkKind::Secondary,
            "possible" => ProjectLinkKind::Possible,
            _ => return Err(MemoryPathRepositoryError::Decode),
        },
    ))
}

fn to_domain_episode_marker_link(
    record: surrealdb::types::Object,
) -> Result<EpisodeMarkerLink, MemoryPathRepositoryError> {
    let obj = record.into_inner();
    let in_rec = obj
        .get("in")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;
    let out_rec = obj
        .get("out")
        .and_then(|v| v.clone().into_t::<surrealdb::types::RecordId>().ok())
        .ok_or(MemoryPathRepositoryError::Decode)?;

    Ok(EpisodeMarkerLink::new(
        EpisodeId::new(record_id_key(&in_rec)).map_err(|_| MemoryPathRepositoryError::Decode)?,
        MarkerId::new(record_id_key(&out_rec)).map_err(|_| MemoryPathRepositoryError::Decode)?,
    ))
}

// ---------------------------------------------------------------------------
// Reconstitution helpers (allow store to rebuild domain objects)
// ---------------------------------------------------------------------------

fn reconstitute_project(
    id: ProjectId,
    name: ProjectName,
    status: ProjectStatus,
    created_at: DateTime<Utc>,
) -> Project {
    // We access private fields via a clever workaround — serialize and deserialize
    let json = serde_json::json!({
        "id": id,
        "name": name,
        "status": status,
        "created_at": created_at,
    });
    serde_json::from_value(json).expect("project reconstitution should always succeed")
}

fn reconstitute_episode(
    id: EpisodeId,
    title: EpisodeTitle,
    source_range: SourceRange,
    created_at: DateTime<Utc>,
) -> Episode {
    let json = serde_json::json!({
        "id": id,
        "title": title,
        "source_range": source_range,
        "created_at": created_at,
    });
    serde_json::from_value(json).expect("episode reconstitution should always succeed")
}

fn reconstitute_marker(
    id: MarkerId,
    display_text: String,
    lookup_key: String,
    created_at: DateTime<Utc>,
) -> Marker {
    let json = serde_json::json!({
        "id": id,
        "display_text": display_text,
        "lookup_key": lookup_key,
        "created_at": created_at,
    });
    serde_json::from_value(json).expect("marker reconstitution should always succeed")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_to_str(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Active => "active",
        ProjectStatus::Paused => "paused",
        ProjectStatus::Archived => "archived",
    }
}

fn link_kind_to_str(k: ProjectLinkKind) -> &'static str {
    match k {
        ProjectLinkKind::Primary => "primary",
        ProjectLinkKind::Secondary => "secondary",
        ProjectLinkKind::Possible => "possible",
    }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

const SCHEMA_MIGRATION_V2: &str = r#"
DEFINE TABLE IF NOT EXISTS project SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name ON project TYPE string;
DEFINE FIELD IF NOT EXISTS status ON project TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON project TYPE datetime;

DEFINE TABLE IF NOT EXISTS episode SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS title ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS source_id ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS start_byte ON episode TYPE int;
DEFINE FIELD IF NOT EXISTS end_byte ON episode TYPE int;
DEFINE FIELD IF NOT EXISTS created_at ON episode TYPE datetime;

DEFINE TABLE IF NOT EXISTS marker SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS display_text ON marker TYPE string;
DEFINE FIELD IF NOT EXISTS lookup_key ON marker TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON marker TYPE datetime;
DEFINE INDEX IF NOT EXISTS marker_lookup_key ON marker FIELDS lookup_key UNIQUE;

DEFINE TABLE IF NOT EXISTS episode_project SCHEMALESS;
DEFINE FIELD IF NOT EXISTS episode_id ON episode_project TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON episode_project TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON episode_project TYPE string;
DEFINE INDEX IF NOT EXISTS episode_project_unique ON episode_project FIELDS episode_id, project_id UNIQUE;

DEFINE TABLE IF NOT EXISTS episode_marker SCHEMALESS;
DEFINE FIELD IF NOT EXISTS episode_id ON episode_marker TYPE string;
DEFINE FIELD IF NOT EXISTS marker_id ON episode_marker TYPE string;
DEFINE INDEX IF NOT EXISTS episode_marker_unique ON episode_marker FIELDS episode_id, marker_id UNIQUE;

UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 2,
    updated_at: time::now()
};
"#;

/// V3: Native SurrealDB graph relation tables.
///
/// These replace the V2 plain-string edge tables for all new memory-path links.
/// V2 tables are preserved as legacy scaffolding and are not deleted.
const SCHEMA_MIGRATION_V3: &str = r#"
DEFINE TABLE IF NOT EXISTS episode_project_relation TYPE RELATION IN episode OUT project ENFORCED SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS kind ON episode_project_relation TYPE string;
DEFINE INDEX IF NOT EXISTS episode_project_rel_unique ON episode_project_relation FIELDS in, out UNIQUE;

DEFINE TABLE IF NOT EXISTS episode_marker_relation TYPE RELATION IN episode OUT marker ENFORCED SCHEMALESS;
DEFINE INDEX IF NOT EXISTS episode_marker_rel_unique ON episode_marker_relation FIELDS in, out UNIQUE;

UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 3,
    updated_at: time::now()
};
"#;
