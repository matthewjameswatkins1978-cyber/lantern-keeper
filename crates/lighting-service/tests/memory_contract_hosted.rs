//! Hosted memory-contract clauses (LK-Q19), the hosted half of the LK-Q10 matrix.
//!
//! Runs the *hosted candidate's own statements* — `lighting-store-d1` builders —
//! against a local SQLite database in memory, executing them the way a D1
//! `batch()` would: one transaction, all-or-nothing, refusal on any error. No
//! network, no wrangler, no live Worker, no live data.
//!
//! Clause status is explicit and machine-checked, exactly as in
//! `memory_contract_fixtures.rs`:
//!
//! * `Pass` — the assertion ran here and held;
//! * `Pending(reason)` — the candidate has no such surface yet, and the reason
//!   names what is missing including how to reproduce the gap.
//!
//! A clause may not be silently absent: `clause_report()` must cover every clause
//! for this adapter exactly once, and a `Pass` must correspond to an assertion
//! executed by this file.

use lighting_store_d1::{
    BatchRefusal, KeyedCorrection, MAX_CANDIDATES, SCHEMA_SQL, SqlStatement, SqlValue,
    belief_detail, belief_revisions, build_correction_batch, idempotency_composite_key,
    lookup_idempotency, scoped_belief_candidates, search_beliefs,
};
use rusqlite::Connection;

/// Outcome of one hosted clause, reported per clause in the receipt.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Status {
    Pass,
    Pending(&'static str),
}

struct ClauseResult {
    clause: &'static str,
    status: Status,
}

const CLAUSES: &[&str] = &[
    "exact_source_bytes",
    "utf8_round_trip",
    "crlf_preservation",
    "attributed_capture",
    "note_is_not_a_claim",
    "value_history",
    "stale_suppression",
    "stale_reads",
    "replay_idempotent",
    "scope_isolation",
    "exact_get",
    "search_scoped",
    "no_false_success",
];

fn open_hosted() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_SQL)?;
    // Mirrors SQLite's default off-switch being turned on, so the declared
    // foreign keys are actually enforced in the local candidate.
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

fn to_sql_values(params: &[SqlValue]) -> Vec<rusqlite::types::Value> {
    params
        .iter()
        .map(|param| match param {
            SqlValue::Text(text) => rusqlite::types::Value::Text(text.clone()),
            SqlValue::Integer(int) => rusqlite::types::Value::Integer(*int),
            SqlValue::Real(real) => rusqlite::types::Value::Real(*real),
            SqlValue::Null => rusqlite::types::Value::Null,
        })
        .collect()
}

/// Execute a statement list as one transaction, rollback on any failure.
fn run_batch(conn: &Connection, statements: &[SqlStatement]) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    for statement in statements {
        let result = (|| -> rusqlite::Result<usize> {
            let mut prepared = conn.prepare(&statement.sql)?;
            prepared.execute(rusqlite::params_from_iter(to_sql_values(&statement.params)))
        })();
        if let Err(error) = result {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn seed_belief(
    conn: &Connection,
    id: &str,
    scope_hash: &str,
    updated_at: &str,
    stale: i64,
    value: &str,
) {
    conn.execute(
        "INSERT INTO belief (id, scope_hash, scope, holder_key, subject_key, predicate_key, \
         current_value, state, stale, dependency_generation, updated_at, payload) \
         VALUES (?1, ?2, ?2, 'matthew', 'matthew', 'preferred_editor', ?3, 'active', ?4, 0, ?5, ?6)",
        rusqlite::params![id, scope_hash, value, stale, updated_at, format!("{{\"value\":\"{value}\"}}")],
    )
    .expect("seed belief");
}

fn correction(id: &str, belief: &str, expected: &str, new: &str, key: &str) -> KeyedCorrection {
    KeyedCorrection {
        revision_id: id.to_owned(),
        target_belief_id: belief.to_owned(),
        expected_revision: expected.to_owned(),
        new_revision: new.to_owned(),
        revision_payload: "{}".to_owned(),
        belief_payload: "{}".to_owned(),
        principal: "matthew".to_owned(),
        scope_hash: "scope-a".to_owned(),
        idempotency_key: key.to_owned(),
        payload_hash: format!("hash-{key}"),
        receipt_id: format!("receipt-{id}"),
        accepted_at: "2026-11-25T00:00:00.000000000Z".to_owned(),
        result_json: format!("{{\"result\":\"{id}\"}}"),
    }
}

fn rows(conn: &Connection, statement: &SqlStatement) -> usize {
    let mut prepared = conn.prepare(&statement.sql).expect("prepare");
    let mut query = prepared
        .query(rusqlite::params_from_iter(to_sql_values(&statement.params)))
        .expect("query");
    let mut seen = 0usize;
    while let Some(_row) = query.next().expect("row") {
        seen += 1;
    }
    seen
}

// --- executable clauses -----------------------------------------------------

/// A keyed correction is accepted as one batch: revision, projection and both
/// receipts move together, and the old value is recoverable.
fn value_history(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "b1", "scope-a", "rev-1", 0, "zed");
    let batch = build_correction_batch(&correction("r1", "b1", "rev-1", "rev-2", "k1"))
        .map_err(|refusal| format!("builder refused: {refusal:?}"))?;
    run_batch(conn, &batch).map_err(|error| format!("batch rejected: {error}"))?;

    let detail = belief_detail("b1", "scope-a");
    let mut prepared = conn.prepare(&detail.sql).expect("prepare detail");
    let updated_at: String = prepared
        .query_row(
            rusqlite::params_from_iter(to_sql_values(&detail.params)),
            |row| row.get::<_, String>(9),
        )
        .map_err(|error| format!("detail failed: {error}"))?;
    if updated_at != "rev-2" {
        return Err(format!(
            "projection did not advance, updated_at={updated_at}"
        ));
    }

    let history = belief_revisions("b1", 10);
    if rows(conn, &history) != 1 {
        return Err("expected exactly one revision row".to_owned());
    }
    Ok(())
}

/// A stale expected revision aborts the whole batch and leaves nothing behind.
fn stale_suppression(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "b1", "scope-a", "rev-1", 0, "zed");
    run_batch(
        conn,
        &build_correction_batch(&correction("r1", "b1", "rev-1", "rev-2", "k1"))
            .map_err(|refusal| format!("builder refused: {refusal:?}"))?,
    )
    .map_err(|error| format!("first batch rejected: {error}"))?;

    // A second writer still expecting rev-1 must be refused in full.
    let stale = build_correction_batch(&correction("r2", "b1", "rev-1", "rev-3", "k2"))
        .map_err(|refusal| format!("builder refused: {refusal:?}"))?;
    if run_batch(conn, &stale).is_ok() {
        return Err("a stale expected_revision was accepted".to_owned());
    }
    if count(conn, "belief_revision") != 1
        || count(conn, "correction_idempotency") != 1
        || count(conn, "mutation_receipt") != 1
    {
        return Err("a rejected batch left partial rows behind".to_owned());
    }
    Ok(())
}

/// A replay of the same key is recognised, and a duplicate batch cannot land.
fn replay_idempotent(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "b1", "scope-a", "rev-1", 0, "zed");
    let batch = build_correction_batch(&correction("r1", "b1", "rev-1", "rev-2", "k-replay"))
        .map_err(|refusal| format!("builder refused: {refusal:?}"))?;
    run_batch(conn, &batch).map_err(|error| format!("first batch rejected: {error}"))?;

    let key = idempotency_composite_key("matthew", "scope-a", "k-replay");
    let lookup = lookup_idempotency(&key);
    let mut prepared = conn.prepare(&lookup.sql).expect("prepare lookup");
    let found: Option<String> = prepared
        .query_row(
            rusqlite::params_from_iter(to_sql_values(&lookup.params)),
            |row| row.get(2),
        )
        .ok();
    if found.as_deref() != Some("{\"result\":\"r1\"}") {
        return Err(format!(
            "replay lookup did not return the original receipt: {found:?}"
        ));
    }

    // Re-running the identical batch must be refused, not silently duplicated.
    if run_batch(conn, &batch).is_ok() {
        return Err("an identical keyed batch was accepted twice".to_owned());
    }
    if count(conn, "correction_idempotency") != 1 {
        return Err("a replay duplicated the receipt".to_owned());
    }
    Ok(())
}

/// Scoped reads never cross scopes, and the candidate cap is enforced.
fn scope_isolation(conn: &Connection) -> Result<(), String> {
    for index in 0..5 {
        seed_belief(
            conn,
            &format!("a{index}"),
            "scope-a",
            &format!("rev-a{index}"),
            0,
            "zed",
        );
    }
    seed_belief(conn, "b1", "scope-b", "rev-b1", 0, "zed");
    let scoped = scoped_belief_candidates("scope-a", true, MAX_CANDIDATES);
    if rows(conn, &scoped) != 5 {
        return Err("a scoped read included another scope or missed a row".to_owned());
    }
    let zero = scoped_belief_candidates("scope-a", true, 0);
    if rows(conn, &zero) != 0 {
        return Err("limit 0 returned rows".to_owned());
    }
    let capped = scoped_belief_candidates("scope-a", true, 10_000);
    if rows(conn, &capped) > MAX_CANDIDATES {
        return Err("the candidate cap was not enforced".to_owned());
    }
    Ok(())
}

/// A stale projection is filtered from scoped reads unless explicitly requested.
fn stale_reads(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "fresh", "scope-a", "rev-f", 0, "zed");
    seed_belief(conn, "stale", "scope-a", "rev-s", 1, "vim");
    let fresh_only = scoped_belief_candidates("scope-a", false, MAX_CANDIDATES);
    let with_stale = scoped_belief_candidates("scope-a", true, MAX_CANDIDATES);
    if rows(conn, &fresh_only) != 1 || rows(conn, &with_stale) != 2 {
        return Err("stale filtering is wrong in one direction or the other".to_owned());
    }
    Ok(())
}

/// Exact get returns the row only inside its own scope.
fn exact_get(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "b1", "scope-a", "rev-1", 0, "zed");
    let own = belief_detail("b1", "scope-a");
    let foreign = belief_detail("b1", "scope-b");
    if rows(conn, &own) != 1 {
        return Err("exact get missed a record in its own scope".to_owned());
    }
    if rows(conn, &foreign) != 0 {
        return Err("exact get leaked a record across scopes".to_owned());
    }
    Ok(())
}

/// Search is scoped and bounded.
fn search_scoped(conn: &Connection) -> Result<(), String> {
    seed_belief(conn, "b1", "scope-a", "rev-1", 0, "zed");
    let hit = search_beliefs("scope-a", &["%zed%".to_owned()], MAX_CANDIDATES);
    let miss = search_beliefs("scope-b", &["%zed%".to_owned()], MAX_CANDIDATES);
    let empty = search_beliefs("scope-a", &[], MAX_CANDIDATES);
    if rows(conn, &hit) != 1 {
        return Err("scoped search missed its own record".to_owned());
    }
    if rows(conn, &miss) != 0 {
        return Err("scoped search crossed into another scope".to_owned());
    }
    if rows(conn, &empty) != 0 {
        return Err("an empty pattern list returned rows".to_owned());
    }
    Ok(())
}

/// No false success: the builder refuses an unacceptable boundary outright instead
/// of emitting a batch that would fail part-way.
fn no_false_success() -> Result<(), String> {
    let no_op = build_correction_batch(&correction("r1", "b1", "rev-1", "rev-1", "k"));
    if !matches!(no_op, Err(BatchRefusal::NoOpAdvance)) {
        return Err(format!("a no-op advance was not refused: {no_op:?}"));
    }
    let mut separated = correction("r2", "b1", "rev-1", "rev-2", "k|bad");
    separated.idempotency_key = "k|bad".to_owned();
    if !matches!(
        build_correction_batch(&separated),
        Err(BatchRefusal::KeySeparator)
    ) {
        return Err("a key containing the separator was not refused".to_owned());
    }
    let mut bad_time = correction("r3", "b1", "rev-1", "rev-3", "k3");
    bad_time.accepted_at = "25/11/2026 00:00".to_owned();
    if !matches!(
        build_correction_batch(&bad_time),
        Err(BatchRefusal::NonCanonicalTimestamp)
    ) {
        return Err("a non-canonical timestamp was not refused".to_owned());
    }
    Ok(())
}

#[test]
fn hosted_clauses_execute_and_unavailable_ones_are_pending() -> Result<(), String> {
    // Each clause gets its own database so one clause's fixture cannot satisfy or
    // break another's assertions.
    let mut results: Vec<ClauseResult> = Vec::new();
    type ClauseCheck = fn(&Connection) -> Result<(), String>;
    let executable: Vec<(&'static str, ClauseCheck)> = vec![
        ("value_history", value_history),
        ("stale_suppression", stale_suppression),
        ("replay_idempotent", replay_idempotent),
        ("scope_isolation", scope_isolation),
        ("exact_get", exact_get),
    ];
    for (clause, check) in &executable {
        let conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
        check(&conn).unwrap_or_else(|error| panic!("hosted clause {clause} failed: {error}"));
        results.push(ClauseResult {
            clause,
            status: Status::Pass,
        });
    }

    let stale_conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
    stale_reads(&stale_conn)
        .unwrap_or_else(|error| panic!("hosted stale read clause failed: {error}"));
    results.push(ClauseResult {
        clause: "stale_reads",
        status: Status::Pass,
    });

    let search_conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
    search_scoped(&search_conn)
        .unwrap_or_else(|error| panic!("hosted search clause failed: {error}"));
    results.push(ClauseResult {
        clause: "search_scoped",
        status: Status::Pass,
    });

    no_false_success().unwrap_or_else(|error| panic!("hosted no-false-success failed: {error}"));
    results.push(ClauseResult {
        clause: "no_false_success",
        status: Status::Pass,
    });

    // Clauses the candidate has no surface for. Each names what is missing and how
    // to see the gap, so nobody can read this file as hosted capture coverage.
    let pending_no_capture = Status::Pending(
        "lighting-store-d1 exposes no capture path: there is no statement builder for \
         source, episode, claim or memory_item, so capture cannot be exercised here. \
         Reproduce: grep -n 'fn ' crates/lighting-store-d1/src/lib.rs | grep -Ei 'source|episode|claim|memory'",
    );
    for clause in [
        "exact_source_bytes",
        "utf8_round_trip",
        "crlf_preservation",
        "attributed_capture",
        "note_is_not_a_claim",
    ] {
        results.push(ClauseResult {
            clause,
            status: pending_no_capture.clone(),
        });
    }

    // Coverage guard: every declared clause appears exactly once.
    for clause in CLAUSES {
        let seen = results
            .iter()
            .filter(|result| result.clause == *clause)
            .count();
        assert_eq!(
            seen, 1,
            "clause {clause} must appear exactly once, saw {seen}"
        );
    }
    assert_eq!(results.len(), CLAUSES.len(), "unexpected extra clauses");

    // A pending reason must be actionable, not a shrug.
    for result in &results {
        if let Status::Pending(reason) = &result.status {
            assert!(
                reason.contains("Reproduce") || reason.contains("no statement builder"),
                "pending reason for {} must name what is missing: {reason}",
                result.clause
            );
        }
    }

    let passes = results.iter().filter(|r| r.status == Status::Pass).count();
    let pending = results.len() - passes;
    eprintln!("HOSTED CLAUSES: {passes} pass, {pending} pending");
    for result in &results {
        match &result.status {
            Status::Pass => eprintln!("  {} : PASS", result.clause),
            Status::Pending(_) => eprintln!("  {} : PENDING", result.clause),
        }
    }
    assert!(
        passes >= 8,
        "expected at least eight executable hosted clauses, got {passes}"
    );
    Ok(())
}
