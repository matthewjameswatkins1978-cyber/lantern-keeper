//! SurrealDB persistence for Lantern's canonical epistemic records.
//!
//! The JSON payload keeps the domain contract engine-independent while the
//! small set of indexed projection fields supports idempotency and safe reads.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    Belief, BeliefId, Claim, ClaimId, DimensionDefinition, EpistemicRepository,
    EpistemicRepositoryError, GraphRelation, MemoryItem, MemoryItemSearch, PredicateDefinition,
    Proposal, Trace,
};
use surrealdb::types::Object;
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealEpistemicError {
    #[error("epistemic query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("epistemic payload could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("epistemic payload could not be decoded: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("epistemic record payload is missing")]
    MissingPayload,
}

#[derive(Clone)]
pub struct SurrealEpistemicRepository {
    store: SurrealStore,
}

impl SurrealEpistemicRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealEpistemicError> {
        self.store
            .query(SCHEMA_MIGRATION_V8)
            .await
            .map(|_| ())
            .map_err(SurrealEpistemicError::Query)
    }
}

#[async_trait]
impl EpistemicRepository for SurrealEpistemicRepository {
    async fn store_claim(&self, claim: Claim) -> Result<Claim, EpistemicRepositoryError> {
        if let Some(existing) = self
            .find_by_key("epistemic_claim", "dedupe_key", &claim.dedupe_key)
            .await
            .map_err(operation)?
        {
            return decode(existing).map_err(operation);
        }
        let payload = serde_json::to_string(&claim)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        self.store
            .query("CREATE epistemic_claim CONTENT { id: $id, payload: $payload, dedupe_key: $dedupe_key, created_at: $created_at }")
            .bind(("id", claim.id.as_str()))
            .bind(("payload", payload))
            .bind(("dedupe_key", claim.dedupe_key.clone()))
            .bind(("created_at", claim.created_at))
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        Ok(claim)
    }

    async fn get_claim(&self, id: &ClaimId) -> Result<Option<Claim>, EpistemicRepositoryError> {
        self.get_by_id("epistemic_claim", id.as_str())
            .await
            .map(|record| record.map(decode).transpose())
            .map_err(operation)?
            .map_err(operation)
    }

    async fn list_claims(
        &self,
        unmapped_only: bool,
    ) -> Result<Vec<Claim>, EpistemicRepositoryError> {
        let records: Vec<Object> = self
            .store
            .query("SELECT * FROM epistemic_claim ORDER BY created_at ASC, id ASC")
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        records
            .into_iter()
            .map(decode)
            .collect::<Result<Vec<Claim>, _>>()
            .map(|claims| {
                if unmapped_only {
                    claims
                        .into_iter()
                        .filter(|claim| claim.predicate_key.is_none())
                        .collect()
                } else {
                    claims
                }
            })
            .map_err(operation)
    }

    async fn store_belief(&self, belief: Belief) -> Result<Belief, EpistemicRepositoryError> {
        let payload = serde_json::to_string(&belief)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        let state = serde_json::to_string(&belief.state)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        self.store
            .query("CREATE belief CONTENT { id: $id, payload: $payload, stale: $stale, state: $state, updated_at: $updated_at }")
            .bind(("id", belief.id.as_str()))
            .bind(("payload", payload))
            .bind(("stale", belief.stale))
            .bind(("state", state))
            .bind(("updated_at", belief.updated_at))
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        Ok(belief)
    }

    async fn get_belief(&self, id: &BeliefId) -> Result<Option<Belief>, EpistemicRepositoryError> {
        self.get_by_id("belief", id.as_str())
            .await
            .map(|record| record.map(decode).transpose())
            .map_err(operation)?
            .map_err(operation)
    }

    async fn list_beliefs(
        &self,
        include_stale: bool,
    ) -> Result<Vec<Belief>, EpistemicRepositoryError> {
        let sql = if include_stale {
            "SELECT * FROM belief ORDER BY updated_at DESC, id ASC"
        } else {
            "SELECT * FROM belief WHERE stale = false ORDER BY updated_at DESC, id ASC"
        };
        let records: Vec<Object> = self
            .store
            .query(sql)
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        records
            .into_iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(operation)
    }

    async fn mark_belief_stale(
        &self,
        id: &BeliefId,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Belief, EpistemicRepositoryError> {
        let mut belief = self
            .get_belief(id)
            .await?
            .ok_or(EpistemicRepositoryError::NotFound)?;
        belief.mark_stale(reason, at);
        let payload = serde_json::to_string(&belief)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        self.store
            .query("UPDATE belief SET payload = $payload, stale = true, updated_at = $updated_at WHERE id = type::record('belief', $id)")
            .bind(("id", id.as_str()))
            .bind(("payload", payload))
            .bind(("updated_at", at))
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        Ok(belief)
    }

    async fn store_memory_item(
        &self,
        item: MemoryItem,
    ) -> Result<MemoryItem, EpistemicRepositoryError> {
        if let Some(existing) = self
            .find_by_key("memory_item", "dedupe_key", &item.dedupe_key)
            .await
            .map_err(operation)?
        {
            return decode(existing).map_err(operation);
        }
        let payload = serde_json::to_string(&item)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        self.store
            .query("CREATE memory_item CONTENT { id: $id, payload: $payload, dedupe_key: $dedupe_key, content: $content, archived: $archived, salience: $salience, created_at: $created_at }")
            .bind(("id", item.id.as_str()))
            .bind(("payload", payload))
            .bind(("dedupe_key", item.dedupe_key.clone()))
            .bind(("content", item.content.clone()))
            .bind(("archived", item.archived))
            .bind(("salience", item.salience))
            .bind(("created_at", item.created_at))
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        Ok(item)
    }

    async fn search_memory_items(
        &self,
        query: &MemoryItemSearch,
    ) -> Result<Vec<MemoryItem>, EpistemicRepositoryError> {
        let mut sql = String::from("SELECT * FROM memory_item WHERE 1 = 1");
        if !query.include_archived {
            sql.push_str(" AND archived = false");
        }
        if query.phrase.is_some() {
            sql.push_str(" AND content CONTAINS $phrase");
        }
        sql.push_str(" ORDER BY salience DESC, created_at DESC, id ASC");
        if query.limit > 0 {
            sql.push_str(" LIMIT $limit");
        }
        let mut request = self.store.query(sql);
        if let Some(phrase) = &query.phrase {
            request = request.bind(("phrase", phrase.clone()));
        }
        if query.limit > 0 {
            request = request.bind(("limit", query.limit));
        }
        let records: Vec<Object> = request
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        records
            .into_iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(operation)
    }

    async fn append_trace(&self, trace: Trace) -> Result<Trace, EpistemicRepositoryError> {
        append_payload(
            &self.store,
            "trace",
            trace.id.as_str(),
            &trace,
            trace.created_at,
        )
        .await
    }

    async fn enqueue_proposal(
        &self,
        proposal: Proposal,
    ) -> Result<Proposal, EpistemicRepositoryError> {
        append_payload(
            &self.store,
            "proposal",
            proposal.id.as_str(),
            &proposal,
            proposal.created_at,
        )
        .await
    }

    async fn store_relation(
        &self,
        relation: GraphRelation,
    ) -> Result<GraphRelation, EpistemicRepositoryError> {
        if let Some(existing) = self
            .find_by_key("memory_relation", "dedupe_key", &relation.dedupe_key)
            .await
            .map_err(operation)?
        {
            return decode(existing).map_err(operation);
        }
        let payload = serde_json::to_string(&relation)
            .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
        self.store
            .query("CREATE memory_relation CONTENT { id: $id, payload: $payload, dedupe_key: $dedupe_key, in_id: $in_id, out_id: $out_id, resolved: $resolved, created_at: $created_at }")
            .bind(("id", relation.id.as_str()))
            .bind(("payload", payload))
            .bind(("dedupe_key", relation.dedupe_key.clone()))
            .bind(("in_id", relation.in_id.clone()))
            .bind(("out_id", relation.out_id.clone()))
            .bind(("resolved", relation.resolved))
            .bind(("created_at", relation.created_at))
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        Ok(relation)
    }

    async fn list_unresolved_relations(
        &self,
    ) -> Result<Vec<GraphRelation>, EpistemicRepositoryError> {
        let records: Vec<Object> = self
            .store
            .query("SELECT * FROM memory_relation WHERE resolved = false ORDER BY created_at ASC, id ASC")
            .await
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?
            .take(0)
            .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
        records
            .into_iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(operation)
    }

    async fn store_predicate_definition(
        &self,
        definition: PredicateDefinition,
    ) -> Result<PredicateDefinition, EpistemicRepositoryError> {
        if let Some(existing) = self
            .find_by_key("predicate_definition", "key", &definition.key)
            .await
            .map_err(operation)?
        {
            return decode(existing).map_err(operation);
        }
        append_registry_definition(
            &self.store,
            "predicate_definition",
            &definition.key,
            &definition,
            definition.created_at,
        )
        .await
    }

    async fn get_predicate_definition(
        &self,
        key: &str,
    ) -> Result<Option<PredicateDefinition>, EpistemicRepositoryError> {
        self.find_by_key("predicate_definition", "key", key)
            .await
            .map_err(operation)?
            .map(decode)
            .transpose()
            .map_err(operation)
    }

    async fn list_predicate_definitions(
        &self,
    ) -> Result<Vec<PredicateDefinition>, EpistemicRepositoryError> {
        list_registry_definitions(&self.store, "predicate_definition").await
    }

    async fn store_dimension_definition(
        &self,
        definition: DimensionDefinition,
    ) -> Result<DimensionDefinition, EpistemicRepositoryError> {
        if let Some(existing) = self
            .find_by_key("dimension_definition", "key", &definition.key)
            .await
            .map_err(operation)?
        {
            return decode(existing).map_err(operation);
        }
        append_registry_definition(
            &self.store,
            "dimension_definition",
            &definition.key,
            &definition,
            Utc::now(),
        )
        .await
    }

    async fn get_dimension_definition(
        &self,
        key: &str,
    ) -> Result<Option<DimensionDefinition>, EpistemicRepositoryError> {
        self.find_by_key("dimension_definition", "key", key)
            .await
            .map_err(operation)?
            .map(decode)
            .transpose()
            .map_err(operation)
    }

    async fn list_dimension_definitions(
        &self,
    ) -> Result<Vec<DimensionDefinition>, EpistemicRepositoryError> {
        list_registry_definitions(&self.store, "dimension_definition").await
    }
}

async fn append_registry_definition<T>(
    store: &SurrealStore,
    table: &str,
    key: &str,
    value: &T,
    created_at: DateTime<Utc>,
) -> Result<T, EpistemicRepositoryError>
where
    T: serde::Serialize + Clone,
{
    let payload = serde_json::to_string(value)
        .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
    let query = format!(
        "CREATE {table} CONTENT {{ id: $id, key: $key, payload: $payload, created_at: $created_at }}"
    );
    store
        .query(query)
        .bind(("id", key.to_owned()))
        .bind(("key", key.to_owned()))
        .bind(("payload", payload))
        .bind(("created_at", created_at))
        .await
        .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
    Ok(value.clone())
}

async fn list_registry_definitions<T>(
    store: &SurrealStore,
    table: &str,
) -> Result<Vec<T>, EpistemicRepositoryError>
where
    T: serde::de::DeserializeOwned,
{
    let query = format!("SELECT * FROM {table} ORDER BY key ASC");
    let records: Vec<Object> = store
        .query(query)
        .await
        .map_err(|error| operation(SurrealEpistemicError::Query(error)))?
        .take(0)
        .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
    records
        .into_iter()
        .map(decode)
        .collect::<Result<Vec<_>, _>>()
        .map_err(operation)
}

async fn append_payload<T>(
    store: &SurrealStore,
    table: &str,
    id: &str,
    value: &T,
    created_at: DateTime<Utc>,
) -> Result<T, EpistemicRepositoryError>
where
    T: serde::Serialize + Clone,
{
    let payload = serde_json::to_string(value)
        .map_err(|error| operation(SurrealEpistemicError::Encode(error)))?;
    let query =
        format!("CREATE {table} CONTENT {{ id: $id, payload: $payload, created_at: $created_at }}");
    store
        .query(query)
        .bind(("id", id))
        .bind(("payload", payload))
        .bind(("created_at", created_at))
        .await
        .map_err(|error| operation(SurrealEpistemicError::Query(error)))?;
    Ok(value.clone())
}

impl SurrealEpistemicRepository {
    async fn find_by_key(
        &self,
        table: &str,
        field: &str,
        value: &str,
    ) -> Result<Option<Object>, SurrealEpistemicError> {
        let query = format!("SELECT * FROM {table} WHERE {field} = $value LIMIT 1");
        self.store
            .query(query)
            .bind(("value", value.to_owned()))
            .await
            .map_err(SurrealEpistemicError::Query)?
            .take(0)
            .map_err(SurrealEpistemicError::Query)
    }

    async fn get_by_id(
        &self,
        table: &str,
        id: &str,
    ) -> Result<Option<Object>, SurrealEpistemicError> {
        let query = format!("SELECT * FROM {table} WHERE id = type::record('{table}', $id)");
        self.store
            .query(query)
            .bind(("id", id.to_owned()))
            .await
            .map_err(SurrealEpistemicError::Query)?
            .take(0)
            .map_err(SurrealEpistemicError::Query)
    }
}

fn decode<T: serde::de::DeserializeOwned>(record: Object) -> Result<T, SurrealEpistemicError> {
    let payload = record
        .into_inner()
        .get("payload")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .ok_or(SurrealEpistemicError::MissingPayload)?;
    serde_json::from_str(&payload).map_err(SurrealEpistemicError::Decode)
}

fn operation(error: impl std::error::Error + Send + Sync + 'static) -> EpistemicRepositoryError {
    EpistemicRepositoryError::Operation(Box::new(error))
}

const SCHEMA_MIGRATION_V8: &str = r#"
DEFINE TABLE IF NOT EXISTS epistemic_claim SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON epistemic_claim TYPE string;
DEFINE FIELD IF NOT EXISTS dedupe_key ON epistemic_claim TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON epistemic_claim TYPE datetime;
DEFINE INDEX IF NOT EXISTS epistemic_claim_dedupe ON epistemic_claim FIELDS dedupe_key UNIQUE;

DEFINE TABLE IF NOT EXISTS belief SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON belief TYPE string;
DEFINE FIELD IF NOT EXISTS stale ON belief TYPE bool;
DEFINE FIELD IF NOT EXISTS state ON belief TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON belief TYPE datetime;
DEFINE INDEX IF NOT EXISTS belief_stale ON belief FIELDS stale;

DEFINE TABLE IF NOT EXISTS memory_item SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON memory_item TYPE string;
DEFINE FIELD IF NOT EXISTS dedupe_key ON memory_item TYPE string;
DEFINE FIELD IF NOT EXISTS content ON memory_item TYPE string;
DEFINE FIELD IF NOT EXISTS archived ON memory_item TYPE bool;
DEFINE FIELD IF NOT EXISTS salience ON memory_item TYPE float;
DEFINE FIELD IF NOT EXISTS created_at ON memory_item TYPE datetime;
DEFINE INDEX IF NOT EXISTS memory_item_dedupe ON memory_item FIELDS dedupe_key UNIQUE;

DEFINE TABLE IF NOT EXISTS trace SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON trace TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON trace TYPE datetime;

DEFINE TABLE IF NOT EXISTS proposal SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON proposal TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON proposal TYPE datetime;

DEFINE TABLE IF NOT EXISTS memory_relation SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS dedupe_key ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS in_id ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS out_id ON memory_relation TYPE string;
DEFINE FIELD IF NOT EXISTS resolved ON memory_relation TYPE bool;
DEFINE FIELD IF NOT EXISTS created_at ON memory_relation TYPE datetime;
DEFINE INDEX IF NOT EXISTS memory_relation_dedupe ON memory_relation FIELDS dedupe_key UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_relation_resolved ON memory_relation FIELDS resolved;

DEFINE TABLE IF NOT EXISTS predicate_definition SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key ON predicate_definition TYPE string;
DEFINE FIELD IF NOT EXISTS payload ON predicate_definition TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON predicate_definition TYPE datetime;
DEFINE INDEX IF NOT EXISTS predicate_definition_key ON predicate_definition FIELDS key UNIQUE;

DEFINE TABLE IF NOT EXISTS dimension_definition SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key ON dimension_definition TYPE string;
DEFINE FIELD IF NOT EXISTS payload ON dimension_definition TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON dimension_definition TYPE datetime;
DEFINE INDEX IF NOT EXISTS dimension_definition_key ON dimension_definition FIELDS key UNIQUE;

UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 8,
    updated_at: time::now()
};
"#;
