//! Additive bounded storage adapter for the hosted seam (LK-HOST-001 v1.1, M1).
//!
//! Implements [`BoundedEpistemicRepository`] over the existing
//! [`SurrealStore`] without changing [`crate::SurrealEpistemicRepository`], its
//! unbounded `list_*` methods, or any existing service call site.
//!
//! Behaviour that differs deliberately from the native path:
//!
//! * every list is keyset-paged with a hard limit and an opaque cursor;
//! * the transition write is one `BEGIN … COMMIT` batch whose statement results
//!   are **checked** (`Response::take_errors`), so a rolled-back batch is
//!   reported as a typed failure instead of a false `Ok`;
//! * the compare-and-set precondition is enforced by a unique `(belief_id, seq)`
//!   row created inside that same batch, so a losing concurrent writer is
//!   refused rather than silently dropped;
//! * the idempotency receipt is written in that batch too, keyed by
//!   principal + scope + payload hash.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lighting_core::{
    Belief, BeliefId, BoundedEpistemicRepository, BoundedError, Claim, GraphRelation,
    IdempotentTransition, Page, PageCursor, PageRequest, TraversalBounds, TraversalClosure,
    WriteOutcome, WriteReceipt, bounded::cursor_sort_key, scope_hash,
};
use surrealdb::types::{Object, Value};

use crate::connection::SurrealStore;

const SCHEMA: &str = "\
DEFINE TABLE IF NOT EXISTS bounded_receipt SCHEMALESS;
DEFINE FIELD IF NOT EXISTS principal ON bounded_receipt TYPE string;
DEFINE FIELD IF NOT EXISTS scope_hash ON bounded_receipt TYPE string;
DEFINE FIELD IF NOT EXISTS payload_hash ON bounded_receipt TYPE string;
DEFINE FIELD IF NOT EXISTS idempotency_key ON bounded_receipt TYPE string;
DEFINE INDEX IF NOT EXISTS bounded_receipt_binding ON bounded_receipt FIELDS principal, scope_hash, idempotency_key UNIQUE;
DEFINE TABLE IF NOT EXISTS bounded_cas SCHEMALESS;
DEFINE FIELD IF NOT EXISTS belief_id ON bounded_cas TYPE string;
DEFINE FIELD IF NOT EXISTS seq ON bounded_cas TYPE int;
DEFINE INDEX IF NOT EXISTS bounded_cas_sequence ON bounded_cas FIELDS belief_id, seq UNIQUE;
";

#[derive(Clone)]
pub struct SurrealBoundedRepository {
    store: SurrealStore,
}

impl SurrealBoundedRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    /// Create the additive tables and indexes. Touches nothing that the native
    /// repositories own.
    pub async fn migrate(&self) -> Result<(), BoundedError> {
        let mut response = self.store.query(SCHEMA).await.map_err(backend)?;
        let errors = response.take_errors();
        if let Some((_, error)) = errors.into_iter().min_by_key(|(index, _)| *index) {
            return Err(BoundedError::StatementRejected {
                statement: 0,
                detail: error.to_string(),
            });
        }
        Ok(())
    }

    // -- paging ------------------------------------------------------------

    async fn claims_page(&self, request: &PageRequest) -> Result<Page<Claim>, BoundedError> {
        let after = self.cursor_position(request)?;
        let mut sql = String::from("SELECT * FROM epistemic_claim");
        if after.is_some() {
            sql.push_str(
                " WHERE (created_at > $after_sort OR (created_at = $after_sort AND id > \
                 type::record('epistemic_claim', $after_id)))",
            );
        }
        sql.push_str(" ORDER BY created_at ASC, id ASC LIMIT $limit");

        let mut query = self.store.query(sql);
        if let Some((sort_key, after_id)) = &after {
            query = query
                .bind(("after_sort", parse_cursor_datetime(sort_key)?))
                .bind(("after_id", after_id.clone()));
        }
        query = query.bind(("limit", request.limit() as i64));

        let mut response = query.await.map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let items: Vec<Claim> = decode_rows(rows)?;
        let next = next_cursor(
            &items,
            request.limit(),
            |claim| cursor_sort_key(claim.created_at),
            |claim| claim.id.as_str().to_owned(),
        );
        Ok(Page::new(items, next, request.limit()))
    }

    async fn beliefs_page(
        &self,
        include_stale: bool,
        request: &PageRequest,
    ) -> Result<Page<Belief>, BoundedError> {
        let after = self.cursor_position(request)?;
        let mut predicates: Vec<String> = Vec::new();
        if !include_stale {
            predicates.push("stale = false".to_owned());
        }
        if after.is_some() {
            predicates.push(
                "(updated_at < $after_sort OR (updated_at = $after_sort AND id > \
                 type::record('belief', $after_id)))"
                    .to_owned(),
            );
        }
        let mut sql = String::from("SELECT * FROM belief");
        if !predicates.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&predicates.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC, id ASC LIMIT $limit");

        let mut query = self.store.query(sql);
        if let Some((sort_key, after_id)) = &after {
            query = query
                .bind(("after_sort", parse_cursor_datetime(sort_key)?))
                .bind(("after_id", after_id.clone()));
        }
        query = query.bind(("limit", request.limit() as i64));

        let mut response = query.await.map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let items: Vec<Belief> = decode_rows(rows)?;
        let next = next_cursor(
            &items,
            request.limit(),
            |belief| cursor_sort_key(belief.updated_at),
            |belief| belief.id.as_str().to_owned(),
        );
        Ok(Page::new(items, next, request.limit()))
    }

    async fn relations_page(
        &self,
        request: &PageRequest,
    ) -> Result<Page<GraphRelation>, BoundedError> {
        let after = self.cursor_position(request)?;
        let mut sql = String::from("SELECT * FROM memory_relation");
        if after.is_some() {
            sql.push_str(
                " WHERE (created_at > $after_sort OR (created_at = $after_sort AND id > \
                 type::record('memory_relation', $after_id)))",
            );
        }
        sql.push_str(" ORDER BY created_at ASC, id ASC LIMIT $limit");

        let mut query = self.store.query(sql);
        if let Some((sort_key, after_id)) = &after {
            query = query
                .bind(("after_sort", parse_cursor_datetime(sort_key)?))
                .bind(("after_id", after_id.clone()));
        }
        query = query.bind(("limit", request.limit() as i64));

        let mut response = query.await.map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let items: Vec<GraphRelation> = decode_rows(rows)?;
        let next = next_cursor(
            &items,
            request.limit(),
            |relation| cursor_sort_key(relation.created_at),
            |relation| relation.id.as_str().to_owned(),
        );
        Ok(Page::new(items, next, request.limit()))
    }

    fn cursor_position(
        &self,
        request: &PageRequest,
    ) -> Result<Option<(String, String)>, BoundedError> {
        match request.cursor() {
            Some(cursor) => cursor.position().map(Some),
            None => Ok(None),
        }
    }

    // -- revision and receipt reads ---------------------------------------

    async fn stored_revision(&self, belief_id: &str) -> Result<Option<u64>, BoundedError> {
        let mut response = self
            .store
            .query(
                "SELECT * FROM bounded_cas WHERE belief_id = $belief_id ORDER BY seq DESC LIMIT 1",
            )
            .bind(("belief_id", belief_id.to_owned()))
            .await
            .map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let fields = row.into_inner();
        let seq = required_i64(&fields, "seq")?;
        Ok(u64::try_from(seq).ok())
    }

    async fn load_receipt(&self, receipt_id: &str) -> Result<Option<ReceiptRow>, BoundedError> {
        let mut response = self
            .store
            .query("SELECT * FROM bounded_receipt WHERE id = type::record('bounded_receipt', $id)")
            .bind(("id", receipt_id.to_owned()))
            .await
            .map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let fields = row.into_inner();
        Ok(Some(ReceiptRow {
            payload_hash: required_string(&fields, "payload_hash")?,
        }))
    }

    async fn belief_by_id(&self, belief_id: &str) -> Result<Option<Belief>, BoundedError> {
        let mut response = self
            .store
            .query("SELECT * FROM belief WHERE id = type::record('belief', $id)")
            .bind(("id", belief_id.to_owned()))
            .await
            .map_err(backend)?;
        let rows: Vec<Object> = response.take(0).map_err(backend)?;
        let beliefs: Vec<Belief> = decode_rows(rows)?;
        Ok(beliefs.into_iter().next())
    }
}

#[async_trait]
impl BoundedEpistemicRepository for SurrealBoundedRepository {
    async fn list_claims_page(&self, request: PageRequest) -> Result<Page<Claim>, BoundedError> {
        self.claims_page(&request).await
    }

    async fn list_beliefs_page(
        &self,
        include_stale: bool,
        request: PageRequest,
    ) -> Result<Page<Belief>, BoundedError> {
        self.beliefs_page(include_stale, &request).await
    }

    async fn list_relations_page(
        &self,
        request: PageRequest,
    ) -> Result<Page<GraphRelation>, BoundedError> {
        self.relations_page(&request).await
    }

    async fn current_revision(&self, belief_id: &BeliefId) -> Result<Option<u64>, BoundedError> {
        self.stored_revision(belief_id.as_str()).await
    }

    async fn apply_idempotent_transition(
        &self,
        transition: IdempotentTransition,
    ) -> Result<WriteReceipt, BoundedError> {
        transition.validate()?;
        let payload_hash = transition.payload_hash();
        let receipt_id = transition.receipt_id();

        // The receipt id is derived from principal + scope + key, so another
        // principal or scope using the same key is a distinct write and can
        // never replay this receipt. Only a changed payload under the same
        // binding is a conflict. Decided LK-HOST-001 v1.1 (Muse, 23 Nov 2026):
        // a global key space is not a safer default, because a cross-principal
        // conflict would itself be an existence oracle and so violate the
        // no-leakage rule.
        if let Some(existing) = self.load_receipt(&receipt_id).await? {
            if existing.payload_hash != payload_hash {
                return Err(BoundedError::PayloadMismatch {
                    key: transition.idempotency_key.clone(),
                });
            }
            return Ok(WriteReceipt {
                receipt_id,
                payload_hash,
                outcome: WriteOutcome::Replayed,
            });
        }

        let belief_id = transition
            .current
            .as_ref()
            .map(|belief| belief.id.clone())
            .or_else(|| transition.previous.as_ref().map(|belief| belief.id.clone()))
            .ok_or_else(|| {
                BoundedError::Backend("transition must name a belief projection".to_owned())
            })?;

        let actual = self.stored_revision(belief_id.as_str()).await?;
        match transition.expected_revision {
            Some(expected) if actual.unwrap_or(0) != expected => {
                return Err(BoundedError::RevisionConflict {
                    expected: Some(expected),
                    actual,
                });
            }
            _ => {}
        }
        let next_seq = actual.unwrap_or(0) + 1;

        let mut statements = vec!["BEGIN TRANSACTION".to_owned()];

        if transition.previous.is_some() {
            statements
                .push("UPDATE belief SET payload = $previous_payload WHERE id = type::record('belief', $previous_id)".to_owned());
        }
        if let Some(current) = transition.current.as_ref() {
            let statement = if transition
                .previous
                .as_ref()
                .is_some_and(|previous| previous.id == current.id)
            {
                "UPDATE belief SET payload = $current_payload, stale = $current_stale, state = $current_state, updated_at = $current_updated_at WHERE id = type::record('belief', $current_id)"
            } else {
                "CREATE belief CONTENT { id: $current_id, payload: $current_payload, stale: $current_stale, state: $current_state, updated_at: $current_updated_at }"
            };
            statements.push(statement.to_owned());
        }
        for index in 0..transition.stale_beliefs.len() {
            statements.push(format!(
                "UPDATE belief SET payload = $stale_{index}_payload, stale = true, updated_at = $stale_{index}_updated_at WHERE id = type::record('belief', $stale_{index}_id)"
            ));
        }
        statements.push(
            "CREATE trace CONTENT { id: $trace_id, payload: $trace_payload, created_at: $trace_created_at }"
                .to_owned(),
        );
        for index in 0..transition.revisions.len() {
            statements.push(format!(
                "CREATE belief_revision CONTENT {{ id: $revision_{index}_id, belief_id: $revision_{index}_belief_id, payload: $revision_{index}_payload, created_at: $revision_{index}_created_at }}"
            ));
        }
        statements.push(
            "CREATE bounded_cas CONTENT { id: $cas_id, belief_id: $cas_belief_id, seq: $cas_seq, receipt_id: $cas_receipt_id }"
                .to_owned(),
        );
        statements.push(
            "CREATE bounded_receipt CONTENT { id: $receipt_id, principal: $principal, scope_hash: $scope_hash, payload_hash: $payload_hash, idempotency_key: $idempotency_key, belief_id: $receipt_belief_id, seq: $receipt_seq, created_at: $receipt_created_at }"
                .to_owned(),
        );
        statements.push("COMMIT TRANSACTION".to_owned());

        let query_text = statements.join(";\n");
        let mut query = self.store.query(query_text);

        if let Some(previous) = transition.previous.as_ref() {
            query = query
                .bind(("previous_id", previous.id.as_str().to_owned()))
                .bind((
                    "previous_payload",
                    serde_json::to_string(previous).map_err(encode)?,
                ));
        }
        if let Some(current) = transition.current.as_ref() {
            query = query
                .bind(("current_id", current.id.as_str().to_owned()))
                .bind((
                    "current_payload",
                    serde_json::to_string(current).map_err(encode)?,
                ))
                .bind(("current_stale", current.stale))
                .bind((
                    "current_state",
                    serde_json::to_string(&current.state).map_err(encode)?,
                ))
                .bind(("current_updated_at", current.updated_at));
        }
        for (index, stale) in transition.stale_beliefs.iter().enumerate() {
            query = query
                .bind((format!("stale_{index}_id"), stale.id.as_str().to_owned()))
                .bind((
                    format!("stale_{index}_payload"),
                    serde_json::to_string(stale).map_err(encode)?,
                ))
                .bind((format!("stale_{index}_updated_at"), stale.updated_at));
        }
        query = query
            .bind(("trace_id", transition.trace.id.as_str().to_owned()))
            .bind((
                "trace_payload",
                serde_json::to_string(&transition.trace).map_err(encode)?,
            ))
            .bind(("trace_created_at", transition.trace.created_at))
            .bind(("cas_id", cas_record_id(belief_id.as_str(), next_seq)))
            .bind(("cas_belief_id", belief_id.as_str().to_owned()))
            .bind(("cas_seq", next_seq as i64))
            .bind(("cas_receipt_id", receipt_id.clone()))
            .bind(("receipt_id", receipt_id.clone()))
            .bind(("principal", transition.principal_id.clone()))
            .bind(("scope_hash", scope_hash(&transition.scope)))
            .bind(("payload_hash", payload_hash.clone()))
            .bind(("idempotency_key", transition.idempotency_key.clone()))
            .bind(("receipt_belief_id", belief_id.as_str().to_owned()))
            .bind(("receipt_seq", next_seq as i64))
            .bind(("receipt_created_at", Utc::now()));
        for (index, revision) in transition.revisions.iter().enumerate() {
            query = query
                .bind((
                    format!("revision_{index}_id"),
                    revision.id.as_str().to_owned(),
                ))
                .bind((
                    format!("revision_{index}_belief_id"),
                    revision.belief_id.as_str().to_owned(),
                ))
                .bind((
                    format!("revision_{index}_payload"),
                    serde_json::to_string(revision).map_err(encode)?,
                ))
                .bind((format!("revision_{index}_created_at"), revision.created_at));
        }

        let mut response = query.await.map_err(backend)?;
        let errors = response.take_errors();
        if !errors.is_empty() {
            // Prefer the root cause over the cascade: SurrealDB reports every
            // statement after a failure as "not executed due to a failed
            // transaction", which would hide the real reason.
            let mut collected: Vec<(usize, String)> = errors
                .into_iter()
                .map(|(index, error)| (index, error.to_string()))
                .collect();
            collected.sort_by_key(|(index, _)| *index);
            let cascade = |detail: &str| detail.contains("failed transaction");
            let root = collected
                .iter()
                .find(|(_, detail)| !cascade(detail))
                .or_else(|| collected.first())
                .cloned()
                .expect("non-empty error list");
            let (statement, detail) = root;
            // A losing concurrent writer is refused, not silently dropped: its
            // sequence row already exists because the competitor committed.
            match self.stored_revision(belief_id.as_str()).await? {
                Some(committed) if committed >= next_seq => {
                    return Err(BoundedError::RevisionConflict {
                        expected: transition.expected_revision,
                        actual: Some(committed),
                    });
                }
                _ => {}
            }
            return Err(BoundedError::StatementRejected { statement, detail });
        }

        Ok(WriteReceipt {
            receipt_id,
            payload_hash,
            outcome: WriteOutcome::Accepted {
                revision_seq: next_seq,
            },
        })
    }

    async fn dependency_closure(
        &self,
        belief_id: &BeliefId,
        bounds: TraversalBounds,
    ) -> Result<TraversalClosure, BoundedError> {
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut collected: Vec<Belief> = Vec::new();
        let mut frontier: Vec<(String, usize)> = vec![(belief_id.as_str().to_owned(), 0)];

        while let Some((current_id, depth)) = frontier.pop() {
            if !visited.insert(current_id.clone()) {
                continue;
            }
            let Some(belief) = self.belief_by_id(&current_id).await? else {
                continue;
            };
            let mut dependencies: Vec<String> = Vec::new();
            if let Some(prior) = belief.lineage.prior_belief_id.clone() {
                dependencies.push(prior);
            }
            dependencies.extend(belief.lineage.derived_from_belief_ids.iter().cloned());
            if depth > bounds.max_depth() {
                return Err(BoundedError::TraversalOverflow {
                    reason: format!("dependency depth exceeds {}", bounds.max_depth()),
                });
            }
            let pending: Vec<String> = dependencies
                .into_iter()
                .filter(|dependency| !visited.contains(dependency))
                .collect();
            if !pending.is_empty() && depth == bounds.max_depth() {
                return Err(BoundedError::TraversalOverflow {
                    reason: format!(
                        "{} further dependencies at depth {depth} would exceed the depth bound",
                        pending.len()
                    ),
                });
            }
            for dependency in pending {
                frontier.push((dependency, depth + 1));
            }
            if visited.len() > bounds.max_nodes() {
                return Err(BoundedError::TraversalOverflow {
                    reason: format!(
                        "dependency nodes exceed the bound of {}",
                        bounds.max_nodes()
                    ),
                });
            }
            collected.push(belief);
        }

        Ok(TraversalClosure::new(collected, bounds.max_depth()))
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

struct ReceiptRow {
    payload_hash: String,
}

fn backend(error: impl std::fmt::Display) -> BoundedError {
    BoundedError::Backend(error.to_string())
}

fn encode(error: serde_json::Error) -> BoundedError {
    BoundedError::Backend(format!("encode failure: {error}"))
}

fn cas_record_id(belief_id: &str, seq: u64) -> String {
    format!("{belief_id}_{seq}")
}

fn parse_cursor_datetime(sort_key: &str) -> Result<DateTime<Utc>, BoundedError> {
    DateTime::parse_from_rfc3339(sort_key)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|_| BoundedError::InvalidCursor)
}

fn next_cursor<T, S, I>(items: &[T], limit: usize, sort: S, id: I) -> Option<PageCursor>
where
    S: Fn(&T) -> String,
    I: Fn(&T) -> String,
{
    if items.len() < limit {
        return None;
    }
    items
        .last()
        .map(|last| PageCursor::issue(&sort(last), &id(last)))
}

fn decode_rows<T>(rows: Vec<Object>) -> Result<Vec<T>, BoundedError>
where
    T: serde::de::DeserializeOwned,
{
    rows.into_iter()
        .map(|row| {
            let fields = row.into_inner();
            let payload = fields
                .get("payload")
                .and_then(|value| value.clone().into_t::<String>().ok())
                .ok_or_else(|| BoundedError::Backend("row has no payload column".to_owned()))?;
            serde_json::from_str(&payload)
                .map_err(|error| BoundedError::Backend(format!("decode failure: {error}")))
        })
        .collect()
}

fn required_string(fields: &BTreeMap<String, Value>, name: &str) -> Result<String, BoundedError> {
    fields
        .get(name)
        .and_then(|value| value.clone().into_t::<String>().ok())
        .ok_or_else(|| BoundedError::Backend(format!("receipt row is missing {name}")))
}

fn required_i64(fields: &BTreeMap<String, Value>, name: &str) -> Result<i64, BoundedError> {
    fields
        .get(name)
        .and_then(|value| value.clone().into_t::<i64>().ok())
        .ok_or_else(|| BoundedError::Backend(format!("row is missing {name}")))
}
