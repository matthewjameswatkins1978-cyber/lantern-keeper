//! Q18-HOSTED — hosted correction path proof on a local candidate.
//!
//! No live calls: this harness executes the REAL `lighting-store-d1`
//! builders and reads against in-memory SQLite (the engine D1 exposes),
//! mirroring the `apps/lighting-worker` handler sequence exactly —
//! replay check, scope-confined point read, typed revision conflict, one
//! atomic batch (`BEGIN … COMMIT`, `ROLLBACK` on any failure = D1
//! `batch()` semantics), receipt with scope on success.
//!
//! Properties proven green:
//! * same-key same-content remember replay returns the original receipt
//!   including `scope_hash`, with no duplicated rows;
//! * changed content under the same key conflicts and writes nothing;
//! * a failed batch is atomic — nothing persisted (forged CAS token and
//!   duplicate revision id, both refused by the real schema/trigger);
//! * correction/current/history/evidence alignment after a success;
//! * denied scope sees nothing at the read layer (point read by
//!   `scope_hash` returns zero rows);
//! * stale suppression holds for `scoped_belief_candidates`.
//!
//! Documented gaps (see the receipt for the gap list):
//! * GAP-1 `search_beliefs` does not filter `stale` (the candidates read
//!   does), and the worker search handler does not post-filter — a stale
//!   belief leaks into search. Reproduction asserted green below;
//!   acceptance form is `#[ignore]`d.
//! * GAP-2 `lantern_correct` fresh responses carry `scope_hash` but the
//!   replayed receipt (stored `result_json`) does not, so fresh and
//!   replayed shapes diverge — contradicting the remember handler's
//!   documented invariant. Reproduction asserted green below; acceptance
//!   form is `#[ignore]`d.

use lighting_store_d1::{
    BatchRefusal, KeyedCorrection, REMEMBER_SCHEMA_SQL, RememberInput, SCHEMA_SQL, SqlStatement,
    SqlValue, belief_detail, belief_revisions, build_correction_batch, build_remember_batch,
    idempotency_composite_key, lookup_idempotency, lookup_remember, memory_note_list,
    scoped_belief_candidates, search_beliefs, source_bytes,
};
use rusqlite::{Connection, Result as SqlResult};

const CANONICAL_T0: &str = "2026-01-01T00:00:00.000000000Z";
const SCOPE_A: &str = "scope-hash-a";
const SCOPE_B: &str = "scope-hash-b";

fn open_schema() -> SqlResult<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_SCHEMA_SQL)?;
    Ok(conn)
}

/// D1 `batch()` semantics: one transaction, any failure rolls everything
/// back. (Same harness shape as the Q17 remember proofs, self-contained.)
fn run_batch(conn: &Connection, statements: &[SqlStatement]) -> SqlResult<()> {
    conn.execute_batch("BEGIN")?;
    let outcome = (|| -> SqlResult<()> {
        for statement in statements {
            let params: Vec<rusqlite::types::Value> = statement
                .params
                .iter()
                .map(|p| match p {
                    SqlValue::Text(text) => rusqlite::types::Value::Text(text.clone()),
                    SqlValue::Integer(int) => rusqlite::types::Value::Integer(*int),
                    SqlValue::Real(real) => rusqlite::types::Value::Real(*real),
                    SqlValue::Null => rusqlite::types::Value::Null,
                })
                .collect();
            let mut prepared = conn.prepare(&statement.sql)?;
            prepared.execute(rusqlite::params_from_iter(params))?;
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            conn.execute_batch("ROLLBACK")?;
            return Err(error);
        }
    }
    Ok(())
}

fn seed_belief(
    conn: &Connection,
    id: &str,
    scope_hash: &str,
    value: &str,
    updated_at: &str,
    stale: bool,
) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO belief (id, scope_hash, scope, holder_key, subject_key, predicate_key,
            current_value, state, stale, updated_at, payload)
         VALUES (?, ?, ?, 'matthew', 'workshop.editor', 'preferred_editor', ?, 'active', ?, ?,
            ?)",
        rusqlite::params![
            id,
            scope_hash,
            r#"{"project":"q18h"}"#,
            value,
            stale as i64,
            updated_at,
            format!(r#"{{"current_value":"{value}"}}"#),
        ],
    )?;
    Ok(())
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count query")
}

fn rows_for(conn: &Connection, statement: &SqlStatement) -> Vec<Vec<String>> {
    let params: Vec<rusqlite::types::Value> = statement
        .params
        .iter()
        .map(|p| match p {
            SqlValue::Text(text) => rusqlite::types::Value::Text(text.clone()),
            SqlValue::Integer(int) => rusqlite::types::Value::Integer(*int),
            SqlValue::Real(real) => rusqlite::types::Value::Real(*real),
            SqlValue::Null => rusqlite::types::Value::Null,
        })
        .collect();
    let mut prepared = conn.prepare(&statement.sql).expect("prepare read");
    let columns = prepared.column_count();
    let mut cursor = prepared
        .query(rusqlite::params_from_iter(params))
        .expect("read query");
    let mut rows = Vec::new();
    while let Some(row) = cursor.next().expect("row") {
        let mut values = Vec::new();
        for index in 0..columns {
            values.push(
                row.get::<_, String>(index)
                    .ok()
                    .or_else(|| row.get::<_, i64>(index).ok().map(|value| value.to_string()))
                    .unwrap_or_default(),
            );
        }
        rows.push(values);
    }
    rows
}

// ---------------------------------------------------------------------------
// Worker handler mirrors (apps/lighting-worker `remember` / `correct`)
// ---------------------------------------------------------------------------

/// Deterministic stand-in for the worker's `remember_payload_hash`: the
/// VALUE is irrelevant to the contract, consistency per (key, content) is
/// what replay compares.
fn remember_hash(composite: &str, content: &str) -> String {
    format!("remember|{composite}|{content}")
}

/// Mirror of the worker's remember handler: replay check first, then one
/// atomic batch (source idempotent, note, receipt carrying `scope_hash`).
fn remember_once(
    conn: &Connection,
    principal: &str,
    content: &str,
    caller_key: &str,
) -> Result<serde_json_free::Receipt, String> {
    let composite = idempotency_composite_key(principal, SCOPE_A, caller_key);
    let hash = remember_hash(&composite, content);
    if let Some(row) = rows_for(conn, &lookup_remember(&composite)).first() {
        // row: key, payload_hash, result_json
        if row[1] != hash {
            return Err("IdempotencyConflict".to_owned());
        }
        return Ok(serde_json_free::Receipt::parse(&row[2], true));
    }
    let source_id: String = content.chars().take(32).collect();
    let result_json = format!(
        r#"{{"receipt_id":"receipt-{caller_key}","source_id":"{source_id}","memory_id":"mem-{caller_key}","envelope_version":"lantern-keeper-write-envelope-v1","accepted_at":"{CANONICAL_T0}","scope_hash":"{SCOPE_A}","replayed":false}}"#
    );
    let remember = RememberInput {
        source_id: source_id.clone(),
        memory_id: format!("mem-{caller_key}"),
        principal: principal.to_owned(),
        scope_hash: SCOPE_A.to_owned(),
        content: content.to_owned(),
        fingerprint: format!("fp-{content}"),
        payload_hash: hash,
        receipt_id: format!("receipt-{caller_key}"),
        accepted_at: CANONICAL_T0.to_owned(),
        result_json: result_json.clone(),
        idempotency_key: caller_key.to_owned(),
    };
    let statements = build_remember_batch(&remember).map_err(|error| format!("{error:?}"))?;
    run_batch(conn, &statements).map_err(|error| format!("batch: {error}"))?;
    Ok(serde_json_free::Receipt::parse(&result_json, false))
}

/// Mirror of the worker's correct handler sequence.
fn correct_once(
    conn: &Connection,
    principal: &str,
    scope_hash: &str,
    belief_id: &str,
    expected_revision: &str,
    new_value: &str,
    caller_key: &str,
) -> Result<serde_json_free::Receipt, String> {
    let composite = idempotency_composite_key(principal, scope_hash, caller_key);
    let hash = format!("correct|{composite}|{belief_id}|{expected_revision}|{new_value}");

    // 1. Replay check.
    if let Some(row) = rows_for(conn, &lookup_idempotency(&composite)).first() {
        // lookup_idempotency returns: key, payload_hash, result_json.
        let stored_hash = row.get(1).cloned().unwrap_or_default();
        let stored_result = row.get(2).cloned().unwrap_or_default();
        if stored_hash != hash {
            return Err("IdempotencyConflict".to_owned());
        }
        return Ok(serde_json_free::Receipt::parse(&stored_result, true));
    }
    // 2. Scope-confined point read (denied scope sees zero rows here).
    let belief_rows = rows_for(conn, &belief_detail(belief_id, scope_hash));
    let belief = belief_rows.first().ok_or("NotFound")?;
    // columns: id, scope, scope_hash, holder_key, subject_key, predicate_key,
    //          current_value, state, stale, updated_at, payload
    let current_revision = belief[9].clone();
    if current_revision != expected_revision {
        return Err("RevisionConflict".to_owned());
    }

    // 3. One atomic batch.
    let accepted_at = CANONICAL_T0;
    let new_token = format!("{expected_revision}+1");
    let result_json = format!(
        r#"{{"receipt_id":"receipt-{caller_key}","belief_id":"{belief_id}","current_revision":"{new_token}","value":"{new_value}","envelope_version":"lantern-keeper-write-envelope-v1","accepted_at":"{accepted_at}","replayed":false}}"#
    );
    let correction = KeyedCorrection {
        revision_id: format!("revision-{caller_key}"),
        target_belief_id: belief_id.to_owned(),
        expected_revision: expected_revision.to_owned(),
        new_revision: new_token.clone(),
        revision_payload: format!(r#"{{"actor":"{principal}","accepted_at":"{accepted_at}"}}"#),
        belief_payload: format!(r#"{{"current_value":"{new_value}"}}"#),
        principal: principal.to_owned(),
        scope_hash: scope_hash.to_owned(),
        idempotency_key: caller_key.to_owned(),
        payload_hash: hash.clone(),
        receipt_id: format!("receipt-{caller_key}"),
        accepted_at: accepted_at.to_owned(),
        result_json: result_json.clone(),
    };
    let statements = build_correction_batch(&correction).map_err(batch_refusal)?;
    run_batch(conn, &statements).map_err(|error| format!("batch: {error}"))?;

    // 4. Fresh success: the worker adds scope_hash to the response
    //    (apps/lighting-worker correct(): `object.insert("scope_hash", …)`).
    let mut with_scope = result_json.clone();
    with_scope.insert_str(
        with_scope.len() - 1,
        &format!(r#","scope_hash":"{scope_hash}"}}"#),
    );
    Ok(serde_json_free::Receipt::parse(&with_scope, false))
}

fn batch_refusal(refusal: BatchRefusal) -> String {
    format!("BatchRefusal: {refusal}")
}

/// Minimal JSON-ish receipt accessor without a JSON dependency: the harness
/// controls every receipt it parses (flat string maps only).
mod serde_json_free {
    #[derive(Debug)]
    pub struct Receipt {
        fields: Vec<(String, String)>,
        pub replayed: bool,
    }

    impl Receipt {
        pub fn parse(text: &str, replayed: bool) -> Self {
            let mut fields = Vec::new();
            let body = text.trim_start_matches('{').trim_end_matches('}');
            for pair in body.split(',') {
                let mut halves = pair.splitn(2, ':');
                let key = halves.next().unwrap_or_default().trim().trim_matches('"');
                let value = halves.next().unwrap_or_default().trim().trim_matches('"');
                fields.push((key.to_owned(), value.to_owned()));
            }
            Receipt { fields, replayed }
        }

        pub fn get(&self, key: &str) -> Option<&str> {
            self.fields
                .iter()
                .find(|(field, _)| field == key)
                .map(|(_, value)| value.as_str())
        }
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// Property (a): same-key same-content remember replay returns the original
/// receipt INCLUDING scope, with no duplicated rows.
#[test]
fn remember_replay_returns_original_receipt_including_scope() {
    let conn = open_schema().expect("schema");
    let first = remember_once(&conn, "pi", "Q18 hosted note", "note-key").expect("first remember");
    let replayed = remember_once(&conn, "pi", "Q18 hosted note", "note-key").expect("replay");

    assert!(!first.replayed, "first call is fresh");
    assert!(replayed.replayed, "second call is a replay");
    assert_eq!(
        replayed.get("scope_hash"),
        Some(SCOPE_A),
        "replayed receipt must carry the scope"
    );
    assert_eq!(
        replayed.get("receipt_id"),
        first.get("receipt_id"),
        "replay returns the ORIGINAL receipt"
    );
    assert_eq!(count(&conn, "source"), 1, "no duplicate source");
    assert_eq!(count(&conn, "memory_item"), 1, "no duplicate note");
    assert_eq!(count(&conn, "remember_idempotency"), 1, "one receipt");
}

/// Property (b): changed content under the same key conflicts and writes
/// nothing.
#[test]
fn changed_content_under_same_key_conflicts_without_writing() {
    let conn = open_schema().expect("schema");
    remember_once(&conn, "pi", "content A", "same-key").expect("first");
    let before_sources = count(&conn, "source");
    let before_notes = count(&conn, "memory_item");

    let outcome = remember_once(&conn, "pi", "content B", "same-key");
    assert_eq!(
        outcome.as_ref().err().map(String::as_str),
        Some("IdempotencyConflict"),
        "a changed payload under the same binding must conflict: {outcome:?}"
    );
    assert_eq!(count(&conn, "source"), before_sources, "no source written");
    assert_eq!(count(&conn, "memory_item"), before_notes, "no note written");
}

/// Property (d): a failed correction batch is atomic — the real schema's
/// CAS trigger refuses a forged token and EVERY statement rolls back.
#[test]
fn failed_correction_batch_persists_nothing() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");

    // Forged expected token: the belief's row says CANONICAL_T0, the batch
    // claims a token that was never issued. The belief_cas_expected trigger
    // must abort the projection advance inside the batch.
    let forged = KeyedCorrection {
        revision_id: "revision-forged".to_owned(),
        target_belief_id: "belief-1".to_owned(),
        expected_revision: "2026-02-01T00:00:00.000000000Z".to_owned(),
        new_revision: "2026-03-01T00:00:00.000000000Z".to_owned(),
        revision_payload: r#"{"actor":"pi"}"#.to_owned(),
        belief_payload: r#"{"current_value":"forged"}"#.to_owned(),
        principal: "pi".to_owned(),
        scope_hash: SCOPE_A.to_owned(),
        idempotency_key: "forged-key".to_owned(),
        payload_hash: "hash-forged".to_owned(),
        receipt_id: "receipt-forged".to_owned(),
        accepted_at: CANONICAL_T0.to_owned(),
        result_json: r#"{"receipt_id":"receipt-forged"}"#.to_owned(),
    };
    let statements = build_correction_batch(&forged).expect("forged batch builds");
    let outcome = run_batch(&conn, &statements);
    assert!(outcome.is_err(), "the CAS trigger must refuse the batch");

    let stored: String = conn
        .query_row(
            "SELECT current_value FROM belief WHERE id = 'belief-1'",
            [],
            |row| row.get(0),
        )
        .expect("belief row");
    assert_eq!(stored, "alpha", "projection must not move");
    assert_eq!(count(&conn, "belief_revision"), 0, "no revision persisted");
    assert_eq!(
        count(&conn, "correction_idempotency"),
        0,
        "no receipt persisted"
    );
    assert_eq!(
        count(&conn, "mutation_receipt"),
        0,
        "no mutation receipt persisted"
    );
}

/// Property (d), second shape: a duplicate revision id fails mid-batch and
/// the earlier statements of the same batch roll back with it.
#[test]
fn duplicate_revision_id_rolls_back_the_whole_batch() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    // Occupy the revision id the batch will use.
    conn.execute(
        "INSERT INTO belief_revision (id, belief_id, from_updated_at, payload, created_at)
         VALUES ('revision-dup', 'belief-1', ?, '{}', ?)",
        rusqlite::params![CANONICAL_T0, CANONICAL_T0],
    )
    .expect("pre-insert revision");

    let correction = KeyedCorrection {
        revision_id: "revision-dup".to_owned(),
        target_belief_id: "belief-1".to_owned(),
        expected_revision: CANONICAL_T0.to_owned(),
        new_revision: "2026-03-01T00:00:00.000000000Z".to_owned(),
        revision_payload: r#"{"actor":"pi"}"#.to_owned(),
        belief_payload: r#"{"current_value":"beta"}"#.to_owned(),
        principal: "pi".to_owned(),
        scope_hash: SCOPE_A.to_owned(),
        idempotency_key: "dup-key".to_owned(),
        payload_hash: "hash-dup".to_owned(),
        receipt_id: "receipt-dup".to_owned(),
        accepted_at: CANONICAL_T0.to_owned(),
        result_json: r#"{"receipt_id":"receipt-dup"}"#.to_owned(),
    };
    let statements = build_correction_batch(&correction).expect("batch builds");
    assert!(
        run_batch(&conn, &statements).is_err(),
        "duplicate revision id must fail the batch"
    );

    let stored: String = conn
        .query_row(
            "SELECT current_value FROM belief WHERE id = 'belief-1'",
            [],
            |row| row.get(0),
        )
        .expect("belief row");
    assert_eq!(stored, "alpha", "projection must roll back with the batch");
    assert_eq!(
        count(&conn, "correction_idempotency"),
        0,
        "receipt must roll back with the batch"
    );
    assert_eq!(
        count(&conn, "mutation_receipt"),
        0,
        "mutation receipt must roll back with the batch"
    );
}

/// Property (e): correction / current / history / evidence alignment after
/// a successful correction, plus denied-scope reads at the point-lookup
/// layer (zero rows, never an existence oracle).
#[test]
fn correction_current_history_and_evidence_align() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    let note = remember_once(&conn, "pi", "evidence note bytes", "evidence-key").expect("remember");

    let fresh = correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "correct-key",
    )
    .expect("correction accepted");
    assert!(!fresh.replayed, "first correction is fresh");
    assert_eq!(
        fresh.get("scope_hash"),
        Some(SCOPE_A),
        "fresh response carries the scope"
    );

    // current: the projection advanced (updated_at token matches the
    // response). The current_value COLUMN divergence is GAP-3, proven
    // separately below.
    let detail = rows_for(&conn, &belief_detail("belief-1", SCOPE_A));
    assert_eq!(
        detail[0][9],
        fresh.get("current_revision").expect("new token"),
        "response token matches the stored projection"
    );

    // history: exactly one revision, consuming the expected token.
    let revisions = rows_for(&conn, &belief_revisions("belief-1", 10));
    assert_eq!(revisions.len(), 1, "one revision for one transition");
    // Column layout of belief_revisions: from_updated_at, created_at, payload.
    assert_eq!(
        revisions[0][0], CANONICAL_T0,
        "revision consumed the expected token"
    );

    // evidence: the remembered source bytes read back scope-confined.
    let source_id = note.get("source_id").expect("source id");
    let evidence = rows_for(&conn, &source_bytes(source_id, SCOPE_A));
    assert_eq!(evidence.len(), 1, "evidence visible in its own scope");
    let denied = rows_for(&conn, &source_bytes(source_id, SCOPE_B));
    assert!(
        denied.is_empty(),
        "evidence invisible under a scope that does not own it"
    );
    let denied_belief = rows_for(&conn, &belief_detail("belief-1", SCOPE_B));
    assert!(
        denied_belief.is_empty(),
        "denied scope sees nothing at the point read (zero rows)"
    );

    // receipts: keyed idempotency + mutation receipt both durable.
    assert_eq!(count(&conn, "correction_idempotency"), 1, "one receipt");
    assert_eq!(count(&conn, "mutation_receipt"), 1, "one mutation receipt");
    let notes = rows_for(&conn, &memory_note_list(SCOPE_A, 10));
    assert_eq!(notes.len(), 1, "note listed in its own scope");
    let denied_notes = rows_for(&conn, &memory_note_list(SCOPE_B, 10));
    assert!(denied_notes.is_empty(), "denied scope lists no notes");
}

/// GAP-3 reproduction (green, deliberate): after a successful correction
/// the belief's `current_value` COLUMN still holds the old value while the
/// `payload` carries the new one — `build_correction_batch` advances
/// `updated_at` + `payload` only, and `belief_detail` SELECTs the stale
/// column. Readers of `current_value` see pre-correction state.
#[test]
fn gap_correction_leaves_stale_current_value_column_reproduction() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "column-gap-key",
    )
    .expect("correction accepted");

    let detail = rows_for(&conn, &belief_detail("belief-1", SCOPE_A));
    assert_eq!(
        detail[0][6], "alpha",
        "GAP-3: current_value column was never advanced by the batch"
    );
    assert!(
        detail[0][10].contains("beta"),
        "GAP-3: the payload DID advance — column and payload diverge"
    );
}

/// GAP-3 acceptance form: `belief_detail.current_value` must match the
/// corrected payload. Ignored until the seam owner fixes the batch (add
/// `current_value = ?` to the projection UPDATE) or drops the column from
/// the read.
#[test]
#[ignore = "GAP-3: build_correction_batch never updates the current_value column (payload advances, column does not)"]
fn correction_updates_the_current_value_column() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "column-gap-key-2",
    )
    .expect("correction accepted");
    let detail = rows_for(&conn, &belief_detail("belief-1", SCOPE_A));
    assert_eq!(
        detail[0][6], "beta",
        "current_value column must carry the correction"
    );
}

/// Property (f), holding half: the scoped candidate read suppresses stale.
#[test]
fn scoped_candidates_suppress_stale_beliefs() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "fresh-1", SCOPE_A, "live", CANONICAL_T0, false).expect("seed");
    seed_belief(&conn, "stale-1", SCOPE_A, "stale", CANONICAL_T0, true).expect("seed");

    let fresh = rows_for(&conn, &scoped_belief_candidates(SCOPE_A, false, 10));
    assert_eq!(fresh.len(), 1, "only the non-stale belief is a candidate");
    assert_eq!(fresh[0][0], "fresh-1");

    let all = rows_for(&conn, &scoped_belief_candidates(SCOPE_A, true, 10));
    assert_eq!(all.len(), 2, "explicit include_stale shows both");
}

/// GAP-1 reproduction (green, deliberate): `search_beliefs` currently
/// returns stale beliefs — the candidates read above does not, and the
/// worker search handler does not post-filter. Documented, not fixed here.
#[test]
fn gap_search_returns_stale_reproduction() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "stale-1", SCOPE_A, "stale value", CANONICAL_T0, true).expect("seed");

    let hits = rows_for(&conn, &search_beliefs(SCOPE_A, &["%stale%".to_owned()], 10));
    assert_eq!(
        hits.len(),
        1,
        "GAP-1: search_beliefs leaks the stale belief (no stale = 0 filter; \
         scoped_belief_candidates has one, the worker search handler does not \
         post-filter). Acceptance form is the ignored test below."
    );
}

/// GAP-1 acceptance form: search must suppress stale like the candidates
/// read does. Ignored until the seam owner fixes the SQL/handler.
#[test]
#[ignore = "GAP-1: search_beliefs has no stale filter (leak proven by gap_search_returns_stale_reproduction)"]
fn search_suppresses_stale_beliefs() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "stale-1", SCOPE_A, "stale value", CANONICAL_T0, true).expect("seed");
    let hits = rows_for(&conn, &search_beliefs(SCOPE_A, &["%stale%".to_owned()], 10));
    assert!(hits.is_empty(), "search must not return stale projections");
}

/// GAP-2 reproduction (green, deliberate): a replayed `lantern_correct`
/// receipt lacks `scope_hash` while the fresh response carries it — the
/// remember handler's documented invariant ("fresh and replayed responses
/// have identical shape apart from `replayed`") does not hold for correct.
#[test]
fn gap_correct_replay_receipt_lacks_scope_reproduction() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "replay-key",
    )
    .expect("first correction");
    let replayed = correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "replay-key",
    )
    .expect("replay");

    assert!(replayed.replayed, "second call replays");
    assert!(
        replayed.get("scope_hash").is_none(),
        "GAP-2: replayed correct receipt has no scope_hash while the fresh \
         response does (worker correct() inserts scope only on the fresh \
         path; replay_response returns the stored result_json verbatim). \
         Acceptance form is the ignored test below."
    );
}

/// GAP-2 acceptance form: fresh and replayed correct receipts must carry
/// the same shape (both including `scope_hash`), per the remember handler's
/// invariant. Ignored until the seam owner fixes the worker.
#[test]
#[ignore = "GAP-2: lantern_correct replayed receipt lacks scope_hash (fresh has it)"]
fn correct_replay_receipt_includes_scope() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "belief-1", SCOPE_A, "alpha", CANONICAL_T0, false).expect("seed");
    correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "replay-key-2",
    )
    .expect("first correction");
    let replayed = correct_once(
        &conn,
        "pi",
        SCOPE_A,
        "belief-1",
        CANONICAL_T0,
        "beta",
        "replay-key-2",
    )
    .expect("replay");
    assert_eq!(
        replayed.get("scope_hash"),
        Some(SCOPE_A),
        "replayed correct receipt must carry the scope"
    );
}
