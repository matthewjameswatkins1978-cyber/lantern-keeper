//! Additive bounded-access contract for the hosted seam (LK-HOST-001 v1.1, M1).
//!
//! This module is deliberately **additive**. It does not change
//! [`crate::EpistemicRepository`] or any existing call site. The existing native
//! surface stays exactly as it is, including the unbounded `list_claims`,
//! `list_beliefs` and `list_relations` methods, which remain correct for local
//! use and are not appropriate for a hosted adapter with a row budget.
//!
//! What this surface provides:
//!
//! * keyset cursor paging with a hard page limit and an opaque, server-controlled
//!   cursor — no scope parameter, no competing scoped-belief query (that fast
//!   path belongs to the scoped-retrieval slice);
//! * one all-or-nothing idempotent transition with an expected-revision
//!   precondition, principal/scope/payload-hash key binding, and typed
//!   nonacceptance instead of a false success receipt;
//! * bounded dependency traversal that reports typed overflow instead of
//!   unbounded work.
//!
//! Nothing here writes on a read path.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Belief, BeliefId, BeliefRevision, Claim, GraphRelation, Scope, Trace, scope_hash};

/// Absolute page limit. v1.1 caps candidates at <=128 total across authorised
/// lanes; a single page can never exceed that.
pub const MAX_PAGE_LIMIT: usize = 128;

/// Default page size when a caller does not choose one.
pub const DEFAULT_PAGE_LIMIT: usize = 32;

/// Initial dependency-traversal depth bound (v1.1 starts at 1–2).
pub const MAX_TRAVERSAL_DEPTH: usize = 2;

/// Initial dependency-traversal fanout bound per level.
pub const MAX_TRAVERSAL_FANOUT: usize = 64;

const CURSOR_SEPARATOR: char = '\u{1f}';

/// Typed nonacceptance for the bounded surface.
///
/// Every variant is a refusal: none of them means "partially applied".
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BoundedError {
    #[error("page limit {limit} is outside the accepted range 1..={MAX_PAGE_LIMIT}")]
    LimitOutOfRange { limit: usize },
    #[error("cursor is not a server-issued cursor")]
    InvalidCursor,
    #[error("idempotency key must not be blank")]
    InvalidIdempotencyKey,
    #[error("principal id must not be blank")]
    InvalidPrincipal,
    #[error("idempotency key {key} was already used with a different payload")]
    PayloadMismatch { key: String },

    #[error("expected revision {expected:?} does not match the stored revision {actual:?}")]
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("statement {statement} was rejected: {detail}")]
    StatementRejected { statement: usize, detail: String },
    #[error("dependency traversal exceeds the bound: {reason}")]
    TraversalOverflow { reason: String },
    #[error("bounded storage operation failed: {0}")]
    Backend(String),
}

/// Opaque, server-issued paging cursor.
///
/// The encoding is intentionally unreadable to clients: callers must treat this
/// as a token. A cursor this server did not issue is rejected as
/// [`BoundedError::InvalidCursor`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageCursor(String);

impl PageCursor {
    /// Build the opaque token for a keyset position.
    pub fn issue(sort_key: &str, tie_break_id: &str) -> Self {
        let payload = format!("{sort_key}{CURSOR_SEPARATOR}{tie_break_id}");
        Self(base64url_encode(payload.as_bytes()))
    }

    /// Read back a keyset position, or refuse a token this server did not issue.
    pub fn position(&self) -> Result<(String, String), BoundedError> {
        let bytes = base64url_decode(&self.0).ok_or(BoundedError::InvalidCursor)?;
        let decoded = String::from_utf8(bytes).map_err(|_| BoundedError::InvalidCursor)?;
        let (sort_key, tie_break_id) = decoded
            .split_once(CURSOR_SEPARATOR)
            .ok_or(BoundedError::InvalidCursor)?;
        if sort_key.is_empty() || tie_break_id.is_empty() {
            return Err(BoundedError::InvalidCursor);
        }
        Ok((sort_key.to_owned(), tie_break_id.to_owned()))
    }

    /// The encoded token, for transport and logs.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for PageCursor {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// One bounded page request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRequest {
    limit: usize,
    cursor: Option<PageCursor>,
}

impl PageRequest {
    /// A first page with an explicit limit.
    pub fn first(limit: usize) -> Result<Self, BoundedError> {
        Self::validated(limit, None)
    }

    /// A follow-on page from a cursor issued by a previous page.
    pub fn after(cursor: PageCursor, limit: usize) -> Result<Self, BoundedError> {
        Self::validated(limit, Some(cursor))
    }

    fn validated(limit: usize, cursor: Option<PageCursor>) -> Result<Self, BoundedError> {
        if limit == 0 || limit > MAX_PAGE_LIMIT {
            return Err(BoundedError::LimitOutOfRange { limit });
        }
        Ok(Self { limit, cursor })
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn cursor(&self) -> Option<&PageCursor> {
        self.cursor.as_ref()
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PAGE_LIMIT,
            cursor: None,
        }
    }
}

/// One bounded page of results.
#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    items: Vec<T>,
    next: Option<PageCursor>,
    limit: usize,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, next: Option<PageCursor>, limit: usize) -> Self {
        Self { items, next, limit }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    pub fn next_cursor(&self) -> Option<&PageCursor> {
        self.next.as_ref()
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// True when the adapter stopped at the requested limit and more rows may
    /// follow. Callers must not infer "no more data" from an absent cursor alone
    /// without also checking this flag on the last page.
    pub fn is_truncated(&self) -> bool {
        self.next.is_some()
    }
}

/// Bounds for a bounded dependency/stale traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraversalBounds {
    max_depth: usize,
    max_nodes: usize,
}

impl TraversalBounds {
    pub fn new(max_depth: usize, max_nodes: usize) -> Result<Self, BoundedError> {
        if max_depth == 0 || max_depth > MAX_TRAVERSAL_DEPTH {
            return Err(BoundedError::TraversalOverflow {
                reason: format!("depth bound {max_depth} is outside 1..={MAX_TRAVERSAL_DEPTH}"),
            });
        }
        if max_nodes == 0 || max_nodes > MAX_TRAVERSAL_FANOUT * MAX_TRAVERSAL_DEPTH {
            return Err(BoundedError::TraversalOverflow {
                reason: format!("node bound {max_nodes} exceeds the accepted maximum"),
            });
        }
        Ok(Self {
            max_depth,
            max_nodes,
        })
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    pub fn max_nodes(&self) -> usize {
        self.max_nodes
    }
}

impl Default for TraversalBounds {
    fn default() -> Self {
        Self {
            max_depth: 1,
            max_nodes: MAX_TRAVERSAL_FANOUT,
        }
    }
}

/// Result of a bounded traversal. `visited` is always complete within the bound:
/// an overflow is reported as [`BoundedError::TraversalOverflow`], never as a
/// short but apparently successful list.
#[derive(Clone, Debug, PartialEq)]
pub struct TraversalClosure {
    beliefs: Vec<Belief>,
    reached_depth: usize,
}

impl TraversalClosure {
    pub fn new(beliefs: Vec<Belief>, reached_depth: usize) -> Self {
        Self {
            beliefs,
            reached_depth,
        }
    }

    pub fn beliefs(&self) -> &[Belief] {
        &self.beliefs
    }

    pub fn into_beliefs(self) -> Vec<Belief> {
        self.beliefs
    }

    pub fn reached_depth(&self) -> usize {
        self.reached_depth
    }
}

/// How an accepted idempotent transition was resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Newly committed; `revision_seq` is the revision this write created.
    Accepted { revision_seq: u64 },
    /// The same key and payload were sent before; no new work was done.
    Replayed,
}

/// Receipt returned for an accepted or replayed idempotent transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteReceipt {
    pub receipt_id: String,
    pub payload_hash: String,
    pub outcome: WriteOutcome,
}

/// One all-or-nothing idempotent transition.
///
/// `expected_revision` is the compare-and-set precondition: `None` creates a new
/// projection, `Some(seq)` replaces only the projection currently at that
/// revision. A stale precondition is refused with
/// [`BoundedError::RevisionConflict`] and leaves no partial state.
#[derive(Clone, Debug)]
pub struct IdempotentTransition {
    pub principal_id: String,
    pub scope: Scope,
    pub idempotency_key: String,
    pub expected_revision: Option<u64>,
    pub previous: Option<Belief>,
    pub current: Option<Belief>,
    pub revisions: Vec<BeliefRevision>,
    pub trace: Trace,
    pub stale_beliefs: Vec<Belief>,
}

impl IdempotentTransition {
    /// Canonical payload hash binding the authenticated principal, the
    /// authorised scope and the envelope content.
    pub fn payload_hash(&self) -> String {
        let mut scope_entries: Vec<String> = self
            .scope
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        scope_entries.sort();
        let envelope = serde_json::json!({
            "v": 1,
            "principal": self.principal_id,
            "scope": scope_entries,
            "scope_hash": scope_hash(&self.scope),
            "key": self.idempotency_key,
            "expected_revision": self.expected_revision,
            "previous": self.previous,
            "current": self.current,
            "revisions": self.revisions,
            "trace": self.trace,
            "stale": self.stale_beliefs,
        });
        sha256_hex(envelope.to_string().as_bytes())
    }

    /// Stable receipt id for this (principal, scope, key) binding.
    pub fn receipt_id(&self) -> String {
        let binding = format!(
            "{}\u{1f}{}\u{1f}{}",
            self.principal_id,
            scope_hash(&self.scope),
            self.idempotency_key
        );
        let digest = sha256_hex(binding.as_bytes());
        format!("rcpt_{}", &digest[..32])
    }

    /// Validate the key binding before any storage work happens.
    pub fn validate(&self) -> Result<(), BoundedError> {
        if self.idempotency_key.trim().is_empty() {
            return Err(BoundedError::InvalidIdempotencyKey);
        }
        if self.principal_id.trim().is_empty() {
            return Err(BoundedError::InvalidPrincipal);
        }
        Ok(())
    }
}

/// Additive bounded storage surface for the hosted seam.
#[async_trait::async_trait]
pub trait BoundedEpistemicRepository: Send + Sync {
    /// One bounded page of claims, keyset-ordered and cursor-continued.
    async fn list_claims_page(&self, request: PageRequest) -> Result<Page<Claim>, BoundedError>;

    /// One bounded page of beliefs, with the native stale filter preserved.
    async fn list_beliefs_page(
        &self,
        include_stale: bool,
        request: PageRequest,
    ) -> Result<Page<Belief>, BoundedError>;

    /// One bounded page of graph relations.
    async fn list_relations_page(
        &self,
        request: PageRequest,
    ) -> Result<Page<GraphRelation>, BoundedError>;

    /// Commit or replay one idempotent transition.
    async fn apply_idempotent_transition(
        &self,
        transition: IdempotentTransition,
    ) -> Result<WriteReceipt, BoundedError>;

    /// Current committed revision of a projection, for compare-and-set callers.
    async fn current_revision(&self, belief_id: &BeliefId) -> Result<Option<u64>, BoundedError>;

    /// Follow belief dependency lineage within explicit bounds.
    async fn dependency_closure(
        &self,
        belief_id: &BeliefId,
        bounds: TraversalBounds,
    ) -> Result<TraversalClosure, BoundedError>;
}

/// RFC3339 helper used for cursor sort keys, so adapters agree on the encoding.
pub fn cursor_sort_key(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

// ---------------------------------------------------------------------------
// hashing and encoding helpers
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(input: &[u8]) -> String {
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0] as u32;
        let second = chunk.get(1).copied().unwrap_or(0) as u32;
        let third = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (first << 16) | (second << 8) | third;
        output.push(BASE64URL[(triple >> 18) as usize & 0x3f] as char);
        output.push(BASE64URL[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            output.push(BASE64URL[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            output.push(BASE64URL[triple as usize & 0x3f] as char);
        }
    }
    output
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut values = Vec::with_capacity(input.len());
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        values.push(value);
    }
    let mut output = Vec::with_capacity(values.len() * 3 / 4);
    for chunk in values.chunks(4) {
        match chunk.len() {
            4 => {
                let triple = ((chunk[0] as u32) << 18)
                    | ((chunk[1] as u32) << 12)
                    | ((chunk[2] as u32) << 6)
                    | chunk[3] as u32;
                output.push((triple >> 16) as u8);
                output.push((triple >> 8) as u8);
                output.push(triple as u8);
            }
            3 => {
                let triple = ((chunk[0] as u32) << 18)
                    | ((chunk[1] as u32) << 12)
                    | ((chunk[2] as u32) << 6);
                output.push((triple >> 16) as u8);
                output.push((triple >> 8) as u8);
            }
            2 => {
                let triple = ((chunk[0] as u32) << 18) | ((chunk[1] as u32) << 12);
                output.push((triple >> 16) as u8);
            }
            _ => return None,
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_foreign_tokens() {
        let cursor = PageCursor::issue("2026-11-23T06:00:00.000000000Z", "belief:abc");
        let (sort_key, id) = cursor.position().expect("issued cursor must decode");
        assert_eq!(sort_key, "2026-11-23T06:00:00.000000000Z");
        assert_eq!(id, "belief:abc");

        let foreign = PageCursor::from(String::new());
        assert_eq!(foreign.position(), Err(BoundedError::InvalidCursor));
        let garbage = PageCursor::from("not a cursor!!".to_owned());
        assert_eq!(garbage.position(), Err(BoundedError::InvalidCursor));
    }

    #[test]
    fn page_limit_is_bounded() {
        assert!(PageRequest::first(0).is_err());
        assert!(PageRequest::first(MAX_PAGE_LIMIT + 1).is_err());
        assert!(PageRequest::first(MAX_PAGE_LIMIT).is_ok());
    }

    #[test]
    fn payload_hash_binds_principal_scope_and_payload() {
        let base = IdempotentTransition {
            principal_id: "pi".to_owned(),
            scope: Scope::from([("project".to_owned(), "lantern-keeper".to_owned())]),
            idempotency_key: "k1".to_owned(),
            expected_revision: None,
            previous: None,
            current: None,
            revisions: Vec::new(),
            trace: Trace {
                id: crate::TraceId::new("t1").expect("trace id"),
                event_type: "contract".to_owned(),
                actor_id: "pi".to_owned(),
                subject_id: None,
                input_ids: Vec::new(),
                output_ids: Vec::new(),
                details: Default::default(),
                created_at: Utc::now(),
            },
            stale_beliefs: Vec::new(),
        };
        let hash = base.payload_hash();

        let mut other_principal = base.clone();
        other_principal.principal_id = "muse".to_owned();
        assert_ne!(hash, other_principal.payload_hash());

        let mut other_scope = base.clone();
        other_scope.scope = Scope::from([("project".to_owned(), "hosted-memory".to_owned())]);
        assert_ne!(hash, other_scope.payload_hash());

        let mut other_key = base.clone();
        other_key.idempotency_key = "k2".to_owned();
        assert_ne!(hash, other_key.payload_hash());

        assert_eq!(hash, base.payload_hash(), "hashing must be deterministic");
    }
}
