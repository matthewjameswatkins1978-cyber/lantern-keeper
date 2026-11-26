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
    BatchRefusal, KeyedCorrection, MAX_CANDIDATES, REMEMBER_ATTR_SCHEMA_SQL, REMEMBER_SCHEMA_SQL,
    RememberInput, SCHEMA_SQL, SqlStatement, SqlValue, belief_detail, belief_revisions,
    build_correction_batch, build_remember_batch, idempotency_composite_key, lookup_idempotency,
    memory_note_list, scoped_belief_candidates, search_beliefs, source_bytes,
};
use rusqlite::Connection;

/// Outcome of one hosted clause, reported per clause in the receipt.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Status {
    Pass,
    /// The only honest way to report a clause this candidate cannot exercise.
    /// Unused while every hosted clause executes; kept so a future gap cannot be
    /// written up as a pass.
    #[allow(dead_code)]
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
    conn.execute_batch(REMEMBER_SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_ATTR_SCHEMA_SQL)?;
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
        new_value: new.to_owned(),
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

// --- capture clauses via the Q17 remember builders --------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn remember(text: &str, key: &str, source_id: &str, memory_id: &str) -> RememberInput {
    RememberInput {
        source_id: source_id.to_owned(),
        memory_id: memory_id.to_owned(),
        principal: "matthew".to_owned(),
        scope_hash: "scope-a".to_owned(),
        actor: "pi:01a206e6".to_owned(),
        content: text.to_owned(),
        fingerprint: sha256_hex(text.as_bytes()),
        payload_hash: format!("payload-{key}"),
        receipt_id: format!("receipt-{key}"),
        accepted_at: "2026-11-26T00:00:00.000000000Z".to_owned(),
        result_json: format!("{{\"key\":\"{key}\"}}"),
        idempotency_key: key.to_owned(),
    }
}

fn store_remember(conn: &Connection, input: &RememberInput) -> Result<(), String> {
    let batch =
        build_remember_batch(input).map_err(|refusal| format!("builder refused: {refusal:?}"))?;
    run_batch(conn, &batch).map_err(|error| format!("remember batch rejected: {error}"))
}

fn capture_case(text: &str, key: &str, scope: &str) -> Result<String, String> {
    let conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
    let input = remember(text, key, &format!("src-{key}"), &format!("note-{key}"));
    store_remember(&conn, &input)?;
    byte_content(&conn, &input.source_id, scope)
}

fn byte_content(conn: &Connection, source_id: &str, scope_hash: &str) -> Result<String, String> {
    let statement = source_bytes(source_id, scope_hash);
    let mut prepared = conn
        .prepare(&statement.sql)
        .map_err(|error| error.to_string())?;
    prepared
        .query_row(
            rusqlite::params_from_iter(to_sql_values(&statement.params)),
            |row| row.get::<_, String>(1),
        )
        .map_err(|error| format!("source_bytes returned nothing: {error}"))
}

/// Exact bytes come back from the stored source, byte for byte.
fn source_bytes_clause() -> Result<(), String> {
    let text = "line one\r\nline two — café 🏮\r\n";
    let stored = capture_case(text, "bytes", "scope-a")?;
    if stored.as_bytes() != text.as_bytes() {
        return Err(format!("stored source bytes differ: {stored:?}"));
    }
    Ok(())
}

/// Multi-byte UTF-8 survives the hosted capture path unchanged.
fn utf8_clause() -> Result<(), String> {
    let text = "café 🏮 Ω";
    let stored = capture_case(text, "utf8", "scope-a")?;
    if stored != text {
        return Err(format!("utf8 changed on capture: {stored:?}"));
    }
    Ok(())
}

/// CRLF is not normalised on the hosted capture path.
fn crlf_clause() -> Result<(), String> {
    let text = "a\r\nb\r\nc";
    let stored = capture_case(text, "crlf", "scope-a")?;
    let carriage_returns = stored
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\r')
        .count();
    if carriage_returns != 2 {
        return Err(format!(
            "expected 2 carriage returns, found {carriage_returns}"
        ));
    }
    Ok(())
}

/// A remembered note is a note, not a claim.
fn note_not_claim_clause() -> Result<(), String> {
    let conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
    let input = remember("maybe use helix", "note", "src-note", "note-1");
    store_remember(&conn, &input)?;

    let notes = memory_note_list("scope-a", MAX_CANDIDATES);
    if rows(&conn, &notes) != 1 {
        return Err("the remembered note is not in the scoped note list".to_owned());
    }
    if count(&conn, "epistemic_claim") != 0 {
        return Err("the remembered note created a claim row".to_owned());
    }
    let foreign = memory_note_list("scope-b", MAX_CANDIDATES);
    if rows(&conn, &foreign) != 0 {
        return Err("the note list crossed scopes".to_owned());
    }
    Ok(())
}
/// Who captured a note survives on the row, is scope-confined, and cannot be
/// borrowed from another scope. The receipt carries the principal; the note row
/// must carry the actor that actually performed the capture.
fn attributed_capture_clause() -> Result<(), String> {
    let conn = open_hosted().map_err(|error| format!("schema failed: {error}"))?;
    let input = remember("captured by pi", "attr", "src-attr", "note-attr");
    if input.actor != "pi:01a206e6" {
        return Err("test fixture lost its actor".to_owned());
    }
    store_remember(&conn, &input)?;

    let actor = |scope: &str| -> Result<Option<String>, String> {
        let mut prepared = conn
            .prepare("SELECT actor FROM memory_item WHERE id = ? AND scope_hash = ?")
            .map_err(|error| error.to_string())?;
        let mut rows = prepared
            .query(rusqlite::params![input.memory_id, scope])
            .map_err(|error| error.to_string())?;
        match rows.next().map_err(|error| error.to_string())? {
            Some(row) => Ok(Some(
                row.get::<_, Option<String>>(0)
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default(),
            )),
            None => Ok(None),
        }
    };

    match actor("scope-a").map_err(|error| format!("actor read failed: {error}"))? {
        Some(found) if found == input.actor => {}
        Some(found) => {
            return Err(format!(
                "stored actor {found:?} is not the capturing actor {:?}",
                input.actor
            ));
        }
        None => return Err("the captured note is not visible in its own scope".to_owned()),
    }
    if actor("scope-b")
        .map_err(|error| format!("cross-scope read failed: {error}"))?
        .is_some()
    {
        return Err("another scope could read the captured note's actor".to_owned());
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

    // Capture clauses are now executable through the Q17 remember builders.
    source_bytes_clause()
        .unwrap_or_else(|error| panic!("hosted clause exact_source_bytes failed: {error}"));
    results.push(ClauseResult {
        clause: "exact_source_bytes",
        status: Status::Pass,
    });
    utf8_clause().unwrap_or_else(|error| panic!("hosted clause utf8_round_trip failed: {error}"));
    results.push(ClauseResult {
        clause: "utf8_round_trip",
        status: Status::Pass,
    });
    crlf_clause().unwrap_or_else(|error| panic!("hosted clause crlf_preservation failed: {error}"));
    results.push(ClauseResult {
        clause: "crlf_preservation",
        status: Status::Pass,
    });
    note_not_claim_clause()
        .unwrap_or_else(|error| panic!("hosted clause note_is_not_a_claim failed: {error}"));
    results.push(ClauseResult {
        clause: "note_is_not_a_claim",
        status: Status::Pass,
    });

    attributed_capture_clause()
        .unwrap_or_else(|error| panic!("hosted clause attributed_capture failed: {error}"));
    results.push(ClauseResult {
        clause: "attributed_capture",
        status: Status::Pass,
    });

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
    assert_eq!(
        passes,
        CLAUSES.len(),
        "every hosted clause must execute, got {passes} of {0}",
        CLAUSES.len()
    );
    assert_eq!(pending, 0, "no hosted clause may remain pending");
    Ok(())
}
