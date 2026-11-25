//! D1/SQLite storage seam for the LK-CUTOVER-001 hosted candidate.
//!
//! This crate owns the **SQL batch construction** for the hosted write path
//! and the bounded scoped reads. It deliberately does not own a database
//! handle: callers hand the produced statements to a D1 `batch()` (or a
//! local SQLite executor in tests) so the same construction is what both
//! the deployed worker and the proof tests exercise.
//!
//! Atomicity model (see `docs/hosted-m1-d1-atomicity.md`):
//! * one logical transition is one D1 batch, which is transactional and
//!   rolls back entirely if any statement fails;
//! * the expected-revision precondition is enforced in the database by two
//!   cooperating guards: UNIQUE on `belief_revision (belief_id,
//!   from_updated_at)` aborts reuse of a consumed token, and the
//!   `belief_cas_expected` trigger aborts a token the database never
//!   issued. A conditional `UPDATE` returning zero rows is deliberately
//!   not used as rollback — it cannot abort anything;
//! * the idempotency receipt is written inside the same boundary and its
//!   primary key aborts a racing duplicate.
//!
//! Batch-construction honesty note: there is no executor or worker in this
//! crate, so "the same statements test and deploy paths use" is a design
//! property (both consume [`build_correction_batch`]), not yet a
//! demonstrated one, and the rusqlite harness emulates D1 batch semantics.
//! Do not report this as D1-proven until the deployed lane runs it.

pub use lighting_core::WRITE_ENVELOPE_VERSION;

/// The checked-in hosted schema, applied by Work at deployment.
pub const SCHEMA_SQL: &str = include_str!("../migrations/0001_hosted_schema.sql");

/* LK-Q17: remember-path migration (additive). Tests apply both; Work
/// applies 0002 after the already-deployed 0001. */
pub const REMEMBER_SCHEMA_SQL: &str = include_str!("../migrations/0002_hosted_remember.sql");

/// Hard bound on candidates returned by a scoped read (v1.1: <=128 total).
pub const MAX_CANDIDATES: usize = 128;

/// Canonical hosted timestamp: RFC3339 UTC, nanosecond precision,
/// trailing Z — exactly `YYYY-MM-DDTHH:MM:SS.fffffffffZ` (30 chars).
/// String ordering is only chronological at fixed width.
pub fn is_canonical_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 30 {
        return false;
    }
    const DIGITS: [usize; 19] = [
        0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 23, 24,
    ];
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[29] != b'Z'
    {
        return false;
    }
    DIGITS.iter().all(|i| bytes[*i].is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Text(String),
    Integer(i64),
    Real(f64),
    Null,
}

impl From<&str> for SqlValue {
    fn from(value: &str) -> Self {
        SqlValue::Text(value.to_owned())
    }
}

impl From<String> for SqlValue {
    fn from(value: String) -> Self {
        SqlValue::Text(value.to_owned())
    }
}

impl From<i64> for SqlValue {
    fn from(value: i64) -> Self {
        SqlValue::Integer(value)
    }
}

#[derive(Debug, Clone)]
pub struct SqlStatement {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl SqlStatement {
    pub fn new(sql: impl Into<String>, params: Vec<SqlValue>) -> Self {
        Self {
            sql: sql.into(),
            params,
        }
    }
}

/// Everything one accepted keyed correction needs, pre-resolved by the
/// service layer. Keeping it a plain struct means the batch builder stays
/// pure and testable without a store.
#[derive(Debug, Clone)]
pub struct KeyedCorrection {
    pub revision_id: String,
    pub target_belief_id: String,
    /// The belief's current `updated_at` token; the CAS precondition.
    pub expected_revision: String,
    /// The `updated_at` token this transition moves the belief to.
    pub new_revision: String,
    pub revision_payload: String,
    pub belief_payload: String,
    pub principal: String,
    pub scope_hash: String,
    pub idempotency_key: String,
    pub payload_hash: String,
    pub receipt_id: String,
    pub accepted_at: String,
    pub result_json: String,
}

/// Why a correction batch was refused before touching the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchRefusal {
    /// The transition advances nowhere; accepting it would burn an
    /// idempotency token for a no-op.
    NoOpAdvance,
    /// A key part contains the `|` composite separator, which would make
    /// two distinct bindings collide on one composite key.
    KeySeparator,
    /// `accepted_at` is not a canonical hosted timestamp; accepting it
    /// would silently break chronological ordering.
    NonCanonicalTimestamp,
    /// The note content is blank: an empty note must never be stored.
    BlankContent,
}

impl std::fmt::Display for BatchRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchRefusal::NoOpAdvance => write!(f, "new_revision equals expected_revision"),
            BatchRefusal::KeySeparator => write!(f, "key part contains '|'"),
            BatchRefusal::NonCanonicalTimestamp => write!(f, "accepted_at is not canonical"),
            BatchRefusal::BlankContent => write!(f, "content is blank"),
        }
    }
}

impl std::error::Error for BatchRefusal {}

/// Build the single atomic batch for an accepted keyed correction.
///
/// Refusals ([`BatchRefusal`]) are returned before any statement is built,
/// so a refused correction can never be half-constructed. Statement order
/// is chosen for the constraint interactions, not for partial application:
/// the batch is all-or-nothing regardless of order, so the guard insert
/// goes first to make the failure mode obvious in logs.
pub fn build_correction_batch(w: &KeyedCorrection) -> Result<Vec<SqlStatement>, BatchRefusal> {
    if w.new_revision == w.expected_revision {
        return Err(BatchRefusal::NoOpAdvance);
    }
    if w.principal.contains('|') || w.scope_hash.contains('|') || w.idempotency_key.contains('|') {
        return Err(BatchRefusal::KeySeparator);
    }
    if !is_canonical_timestamp(&w.accepted_at) {
        return Err(BatchRefusal::NonCanonicalTimestamp);
    }
    Ok(vec![
        // 1. CAS guard + immutable revision history. UNIQUE(belief_id,
        //    from_updated_at) aborts the batch when the transition is stale
        //    or a duplicate; the belief_cas_expected trigger aborts a
        //    forged token. Both fire before anything else is written.
        SqlStatement::new(
            "INSERT INTO belief_revision (id, belief_id, from_updated_at, payload, created_at) VALUES (?, ?, ?, ?, ?)",
            vec![
                w.revision_id.clone().into(),
                w.target_belief_id.clone().into(),
                w.expected_revision.clone().into(),
                w.revision_payload.clone().into(),
                w.accepted_at.clone().into(),
            ],
        ),
        // 2. Advance the governed projection.
        SqlStatement::new(
            "UPDATE belief SET updated_at = ?, payload = ? WHERE id = ?",
            vec![
                w.new_revision.clone().into(),
                w.belief_payload.clone().into(),
                w.target_belief_id.clone().into(),
            ],
        ),
        // 3. Idempotency receipt inside the boundary. PRIMARY KEY aborts a
        //    racing writer that reuses the same composite key.
        SqlStatement::new(
            "INSERT INTO correction_idempotency (key, principal, scope_hash, idempotency_key, payload_hash, result_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![
                idempotency_composite_key(&w.principal, &w.scope_hash, &w.idempotency_key).into(),
                w.principal.clone().into(),
                w.scope_hash.clone().into(),
                w.idempotency_key.clone().into(),
                w.payload_hash.clone().into(),
                w.result_json.clone().into(),
                w.accepted_at.clone().into(),
            ],
        ),
        // 4. Mutation receipt, also inside the boundary. `staged` counts
        //    governed domain writes in this batch (the projection advance
        //    = 1); receipts are bookkeeping, not staged.
        SqlStatement::new(
            "INSERT INTO mutation_receipt (receipt_id, envelope_version, principal, scope_hash, payload_hash, staged, accepted_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![
                w.receipt_id.clone().into(),
                WRITE_ENVELOPE_VERSION.into(),
                w.principal.clone().into(),
                w.scope_hash.clone().into(),
                w.payload_hash.clone().into(),
                1i64.into(),
                w.accepted_at.clone().into(),
            ],
        ),
    ])
}

/// Composite idempotency key: principal + scope hash + caller key.
///
/// Deliberately *binding*, not global contention: the same caller key under
/// a different principal or scope is a distinct write and can never replay
/// this receipt. A cross-principal conflict would itself be an existence
/// oracle, so a global key space would violate the no-leakage rule.
pub fn idempotency_composite_key(principal: &str, scope_hash: &str, caller_key: &str) -> String {
    format!("{principal}|{scope_hash}|{caller_key}")
}

/// Scoped belief candidate read: bounded, indexed, zero-write.
///
/// `limit == 0` returns no rows — the hosted surface never represents an
/// unbounded result. Anything above [`MAX_CANDIDATES`] is clamped.
/// Ordering is chronological only for canonical timestamps (see schema).
pub fn scoped_belief_candidates(
    scope_hash: &str,
    include_stale: bool,
    limit: usize,
) -> SqlStatement {
    let effective = limit.min(MAX_CANDIDATES);
    if effective == 0 {
        return SqlStatement::new("SELECT id, payload, updated_at FROM belief WHERE 0", vec![]);
    }
    if include_stale {
        SqlStatement::new(
            "SELECT id, payload, updated_at FROM belief WHERE scope_hash = ? ORDER BY updated_at DESC, id ASC LIMIT ?",
            vec![scope_hash.into(), (effective as i64).into()],
        )
    } else {
        SqlStatement::new(
            "SELECT id, payload, updated_at FROM belief WHERE scope_hash = ? AND stale = 0 ORDER BY updated_at DESC, id ASC LIMIT ?",
            vec![scope_hash.into(), (effective as i64).into()],
        )
    }
}

/// Scoped search: LIKE patterns bound as parameters (never interpolated),
/// scoped by index, clamped to [`MAX_CANDIDATES`]. Zero-write. Empty
/// patterns return no rows rather than an unscoped scan.
pub fn search_beliefs(scope_hash: &str, patterns: &[String], limit: usize) -> SqlStatement {
    let effective = limit.min(MAX_CANDIDATES);
    if effective == 0 || patterns.is_empty() {
        return SqlStatement::new("SELECT id, payload, updated_at FROM belief WHERE 0", vec![]);
    }
    let mut sql =
        String::from("SELECT id, payload, updated_at FROM belief WHERE scope_hash = ? AND (");
    for index in 0..patterns.len() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("(payload LIKE ? COLLATE NOCASE)");
    }
    sql.push_str(") ORDER BY updated_at DESC, id ASC LIMIT ?");
    let mut params = vec![SqlValue::Text(scope_hash.to_owned())];
    for pattern in patterns {
        params.push(SqlValue::Text(pattern.clone()));
    }
    params.push(SqlValue::Integer(effective as i64));
    SqlStatement::new(sql, params)
}

/// Point lookup of one belief inside its scope. Read-only; a missing row is
/// a typed not-found upstream, never an existence oracle across scopes.
pub fn belief_detail(belief_id: &str, scope_hash: &str) -> SqlStatement {
    SqlStatement::new(
        "SELECT id, scope, scope_hash, holder_key, subject_key, predicate_key, current_value, state, stale, updated_at, payload FROM belief WHERE id = ? AND scope_hash = ? LIMIT 1",
        vec![belief_id.into(), scope_hash.into()],
    )
}

/// Revision lineage for one belief, newest first, clamped (max 64).
/// `from_updated_at` is the consumed token; `created_at` orders history.
pub fn belief_revisions(belief_id: &str, limit: usize) -> SqlStatement {
    let effective = limit.clamp(1, 64);
    SqlStatement::new(
        "SELECT from_updated_at, created_at, payload FROM belief_revision WHERE belief_id = ? ORDER BY created_at DESC, id DESC LIMIT ?",
        vec![belief_id.into(), (effective as i64).into()],
    )
}

/// Replay lookup for a keyed write. Returns the stored result row if this
/// exact composite key was already accepted.
pub fn lookup_idempotency(composite_key: &str) -> SqlStatement {
    SqlStatement::new(
        "SELECT key, payload_hash, result_json FROM correction_idempotency WHERE key = ?",
        vec![composite_key.into()],
    )
}

/* ---------------------------------------------------------------------------
 * LK-Q17 hosted capture: ordinary remember (note, not claim).
 *
 * A remembered note is a Source row (kind 'note', byte-exact content) plus
 * a scoped memory_item row pointing at it. It is NEVER an epistemic_claim:
 * the schema has no claim write here by construction, so ordinary capture
 * cannot silently promote a note into a verified claim.
 *
 * Replay uses remember_idempotency with the same binding rule as
 * corrections (principal|scope|caller key). Source/memory rows use
 * deterministic ids so a replayed batch collides benignly instead of
 * duplicating, but the receipt lookup short-circuits first.
 * ---------------------------------------------------------------------------
 */

/// Everything one accepted keyed remember needs, pre-resolved by the worker.
#[derive(Debug, Clone)]
pub struct RememberInput {
    pub source_id: String,
    pub memory_id: String,
    pub principal: String,
    pub scope_hash: String,
    pub content: String,
    /// Raw content fingerprint (sha256 of bytes): identifies WHAT was
    /// remembered, independent of who/where/with-what-key. Two keys over
    /// the same bytes MUST yield the same fingerprint.
    pub fingerprint: String,
    /// Replay envelope digest (envelope|composite|content): identifies the
    /// keyed write for idempotency. NEVER reuse as the content fingerprint.
    pub payload_hash: String,
    pub receipt_id: String,
    pub accepted_at: String,
    pub result_json: String,
    pub idempotency_key: String,
}

/// Build the single atomic batch for an accepted keyed remember.
///
/// Statement 1 (source) uses INSERT ... ON CONFLICT DO NOTHING: the same
/// bytes re-remembered resolve to the same source row instead of failing.
/// Statements 2-3 carry the write's identity and abort duplicates.
pub fn build_remember_batch(w: &RememberInput) -> Result<Vec<SqlStatement>, BatchRefusal> {
    if w.content.trim().is_empty() {
        return Err(BatchRefusal::BlankContent);
    }
    if w.principal.contains('|') || w.scope_hash.contains('|') || w.idempotency_key.contains('|') {
        return Err(BatchRefusal::KeySeparator);
    }
    if !is_canonical_timestamp(&w.accepted_at) {
        return Err(BatchRefusal::NonCanonicalTimestamp);
    }
    Ok(vec![
        SqlStatement::new(
            "INSERT INTO source (id, kind, title, content, fingerprint, created_at) VALUES (?, 'note', ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING",
            vec![
                w.source_id.clone().into(),
                w.receipt_id.clone().into(),
                w.content.clone().into(),
                w.fingerprint.clone().into(),
                w.accepted_at.clone().into(),
            ],
        ),
        SqlStatement::new(
            "INSERT INTO memory_item (id, scope_hash, content, kind, salience, archived, payload, created_at, source_id) VALUES (?, ?, ?, 'note', 0.0, 0, ?, ?, ?)",
            vec![
                w.memory_id.clone().into(),
                w.scope_hash.clone().into(),
                w.content.clone().into(),
                w.content.clone().into(),
                w.accepted_at.clone().into(),
                w.source_id.clone().into(),
            ],
        ),
        SqlStatement::new(
            "INSERT INTO remember_idempotency (key, principal, scope_hash, idempotency_key, payload_hash, result_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![
                idempotency_composite_key(&w.principal, &w.scope_hash, &w.idempotency_key).into(),
                w.principal.clone().into(),
                w.scope_hash.clone().into(),
                w.idempotency_key.clone().into(),
                w.payload_hash.clone().into(),
                w.result_json.clone().into(),
                w.accepted_at.clone().into(),
            ],
        ),
    ])
}

/// Replay lookup for a keyed remember. Same shape as corrections.
pub fn lookup_remember(composite_key: &str) -> SqlStatement {
    SqlStatement::new(
        "SELECT key, payload_hash, result_json FROM remember_idempotency WHERE key = ?",
        vec![composite_key.into()],
    )
}

/// Bounded scoped note list, newest first. Zero-write.
pub fn memory_note_list(scope_hash: &str, limit: usize) -> SqlStatement {
    let effective = limit.min(MAX_CANDIDATES);
    if effective == 0 {
        return SqlStatement::new(
            "SELECT id, content, created_at FROM memory_item WHERE 0",
            vec![],
        );
    }
    SqlStatement::new(
        "SELECT id, content, created_at FROM memory_item WHERE scope_hash = ? AND archived = 0 ORDER BY created_at DESC, id ASC LIMIT ?",
        vec![scope_hash.into(), (effective as i64).into()],
    )
}

/// Exact source-bytes point read, scope-confined through the owning note.
/// A source with no note in the caller's scope is invisible (no cross-scope
/// oracle), and a missing row is a typed not-found upstream.
pub fn source_bytes(source_id: &str, scope_hash: &str) -> SqlStatement {
    SqlStatement::new(
        "SELECT s.id, s.content, s.kind, s.fingerprint, s.created_at FROM source s INNER JOIN memory_item m ON m.source_id = s.id WHERE s.id = ? AND m.scope_hash = ? LIMIT 1",
        vec![source_id.into(), scope_hash.into()],
    )
}
