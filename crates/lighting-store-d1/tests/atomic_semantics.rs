//! Atomic-semantics proofs for the hosted write boundary, run against real
//! SQLite (the engine D1 exposes). These prove the batch construction, not
//! mocks: constraint aborts, all-or-nothing rollback, and idempotent replay.
//!
//! Honesty scope: the "competing" tests are serialised on one connection —
//! they prove contention handling (exactly-one via guards), not true
//! concurrency. The rusqlite harness emulates D1 `batch()` semantics; this
//! is not D1-proven until the deployed lane runs it.

use lighting_store_d1::{
    BatchRefusal, KeyedCorrection, MAX_CANDIDATES, SCHEMA_SQL, SqlStatement, SqlValue,
    WRITE_ENVELOPE_VERSION, build_correction_batch, idempotency_composite_key, lookup_idempotency,
    scoped_belief_candidates,
};
use rusqlite::{Connection, Result as SqlResult};

const CANONICAL_T0: &str = "2026-01-01T00:00:00.000000000Z";

fn open_schema() -> SqlResult<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

fn seed_belief(
    conn: &Connection,
    id: &str,
    scope: &str,
    updated_at: &str,
    stale: bool,
) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO belief (id, scope_hash, scope, holder_key, subject_key, predicate_key, current_value, state, stale, dependency_generation, updated_at, payload) VALUES (?1, ?2, ?3, 'h', 's', 'p', 'v', 'active', ?4, 0, ?5, '{}')",
        rusqlite::params![id, scope, scope, stale as i32, updated_at],
    )?;
    Ok(())
}

fn run_batch(conn: &Connection, statements: &[SqlStatement]) -> SqlResult<()> {
    // Mirrors D1 `batch()`: one transaction, rolled back entirely on any error.
    // The ROLLBACK on failure is load-bearing: without it an aborted batch
    // would leave its partial statements visible on this connection, which a
    // real D1 batch never does.
    conn.execute_batch("BEGIN")?;
    let outcome = run_statements(conn, statements);
    match outcome {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            conn.execute_batch("ROLLBACK")?;
            return Err(e);
        }
    }
    Ok(())
}

fn run_statements(conn: &Connection, statements: &[SqlStatement]) -> SqlResult<()> {
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
}

fn correction(
    id: &str,
    belief: &str,
    expected: &str,
    new: &str,
    caller_key: &str,
) -> KeyedCorrection {
    KeyedCorrection {
        revision_id: id.to_owned(),
        target_belief_id: belief.to_owned(),
        expected_revision: expected.to_owned(),
        new_revision: new.to_owned(),
        revision_payload: "{}".to_owned(),
        belief_payload: "{}".to_owned(),
        principal: "pi".to_owned(),
        scope_hash: "scope-a".to_owned(),
        idempotency_key: caller_key.to_owned(),
        payload_hash: format!("hash-{caller_key}"),
        receipt_id: format!("receipt-{id}"),
        accepted_at: CANONICAL_T0.to_owned(),
        result_json: format!("{{\"result\":\"{id}\"}}"),
    }
}

fn batch_of(write: &KeyedCorrection) -> Vec<SqlStatement> {
    build_correction_batch(write).expect("test correction must build")
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count query")
}

fn belief_updated_at(conn: &Connection, id: &str) -> String {
    conn.query_row("SELECT updated_at FROM belief WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .expect("belief")
}

#[test]
fn stale_expected_revision_aborts_whole_batch() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");

    let first = correction("r1", "b1", "rev-1", "rev-2", "k1");
    run_batch(&conn, &batch_of(&first)).expect("first accepted");

    // A second writer still expecting rev-1 must abort entirely.
    let stale = correction("r2", "b1", "rev-1", "rev-3", "k2");
    assert!(
        run_batch(&conn, &batch_of(&stale)).is_err(),
        "stale expected_revision must abort the batch"
    );

    // Nothing from the rejected batch may persist.
    assert_eq!(count(&conn, "belief_revision"), 1, "no orphan revision");
    assert_eq!(
        count(&conn, "correction_idempotency"),
        1,
        "no orphan receipt"
    );
    assert_eq!(
        count(&conn, "mutation_receipt"),
        1,
        "no orphan mutation receipt"
    );
    assert_eq!(
        belief_updated_at(&conn, "b1"),
        "rev-2",
        "projection must not move on a rejected transition"
    );
}

#[test]
fn forged_expected_revision_aborts_with_nothing_moved() {
    // The bypass C1 demonstrated: UNIQUE alone cannot detect a token the
    // database never issued. The CAS trigger must abort it.
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");

    let forged = correction("r9", "b1", "rev-never-issued", "rev-9", "k9");
    assert!(
        run_batch(&conn, &batch_of(&forged)).is_err(),
        "forged expected_revision must abort the batch"
    );
    assert_eq!(count(&conn, "belief_revision"), 0, "no orphan revision");
    assert_eq!(
        count(&conn, "correction_idempotency"),
        0,
        "no orphan receipt"
    );
    assert_eq!(
        count(&conn, "mutation_receipt"),
        0,
        "no orphan mutation receipt"
    );
    assert_eq!(
        belief_updated_at(&conn, "b1"),
        "rev-1",
        "projection must not move on a forged token"
    );
}

#[test]
fn competing_serialised_corrections_commit_exactly_one() {
    // Serialised on one connection: proves contention handling via guards,
    // not true concurrency (see module docs).
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");

    let writer_a = correction("ra", "b1", "rev-1", "rev-2", "ka");
    let writer_b = correction("rb", "b1", "rev-1", "rev-9", "kb");

    let a = run_batch(&conn, &batch_of(&writer_a));
    let b = run_batch(&conn, &batch_of(&writer_b));

    assert!(a.is_ok(), "first writer accepted");
    assert!(b.is_err(), "competing writer must lose via CAS guard");
    assert_eq!(count(&conn, "belief_revision"), 1);
    assert_eq!(count(&conn, "correction_idempotency"), 1);
    assert_eq!(belief_updated_at(&conn, "b1"), "rev-2");
}

#[test]
fn duplicate_key_race_aborts_and_keeps_original() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");

    let first = correction("r1", "b1", "rev-1", "rev-2", "k-dup");
    run_batch(&conn, &batch_of(&first)).expect("first accepted");

    // A racing writer reusing the same composite key under a fresh belief
    // revision still collides on the idempotency primary key.
    seed_belief(&conn, "b2", "scope-a", "rev-x", false).expect("seed");
    let racer = correction("r9", "b2", "rev-x", "rev-y", "k-dup");
    assert!(
        run_batch(&conn, &batch_of(&racer)).is_err(),
        "duplicate composite key must abort"
    );
    assert_eq!(
        count(&conn, "correction_idempotency"),
        1,
        "original receipt kept"
    );
}

#[test]
fn replay_lookup_finds_original_receipt() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");
    let write = correction("r1", "b1", "rev-1", "rev-2", "k-replay");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    // A second batch under the same key must fail, not write: replay is
    // lookup-then-return, never re-execute.
    let retry = correction("r1-again", "b1", "rev-2", "rev-3", "k-replay");
    assert!(
        run_batch(&conn, &batch_of(&retry)).is_err(),
        "replay must not re-execute"
    );
    assert_eq!(count(&conn, "belief_revision"), 1);
    assert_eq!(belief_updated_at(&conn, "b1"), "rev-2");

    let key = idempotency_composite_key("pi", "scope-a", "k-replay");
    let found: Option<String> = {
        let mut prepared = conn
            .prepare(&lookup_idempotency(&key).sql)
            .expect("prepare");
        let mut rows = prepared.query(rusqlite::params![key]).expect("query");
        rows.next()
            .expect("row")
            .map(|row| row.get(2).expect("result_json"))
    };
    assert_eq!(found, Some("{\"result\":\"r1\"}".to_owned()));
}

#[test]
fn mutation_receipt_carries_envelope_version() {
    let conn = open_schema().expect("schema");
    seed_belief(&conn, "b1", "scope-a", "rev-1", false).expect("seed");
    let write = correction("r1", "b1", "rev-1", "rev-2", "k1");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    let version: String = conn
        .query_row(
            "SELECT envelope_version FROM mutation_receipt WHERE receipt_id = 'receipt-r1'",
            [],
            |r| r.get(0),
        )
        .expect("receipt");
    assert_eq!(version, WRITE_ENVELOPE_VERSION);
}

fn rows_for(conn: &Connection, statement: &SqlStatement) -> usize {
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
    let mut prepared = conn.prepare(&statement.sql).expect("prepare");
    let mut rows = prepared
        .query(rusqlite::params_from_iter(params))
        .expect("query");
    let mut seen = 0usize;
    while let Some(_row) = rows.next().expect("row") {
        seen += 1;
    }
    seen
}

#[test]
fn scoped_reads_are_bounded_and_scoped() {
    let conn = open_schema().expect("schema");
    for i in 0..5 {
        seed_belief(
            &conn,
            &format!("b{i}"),
            "scope-a",
            &format!("rev-{i}"),
            false,
        )
        .expect("seed");
    }
    // One genuinely stale row: the stale filter must actually exclude it,
    // else this test cannot fail.
    conn.execute(
        "INSERT INTO belief (id, scope_hash, scope, holder_key, subject_key, predicate_key, current_value, state, stale, dependency_generation, updated_at, payload) VALUES ('stale-1', 'scope-a', 'scope-a', 'h', 's', 'p', 'v', 'active', 1, 0, 'rev-s', '{}')",
        [],
    )
    .expect("stale seed");
    seed_belief(&conn, "other", "scope-b", "rev-z", false).expect("seed");

    // limit 0 -> no rows, never unbounded.
    let zero = scoped_belief_candidates("scope-a", true, 0);
    assert_eq!(rows_for(&conn, &zero), 0, "limit 0 must return no rows");

    // Scope isolation: only scope-a rows (5 fresh + 1 stale).
    let scoped = scoped_belief_candidates("scope-a", true, 128);
    assert_eq!(
        rows_for(&conn, &scoped),
        6,
        "cross-scope rows must not be selected"
    );

    // Stale exclusion when asked: the filter must remove exactly the stale row.
    let fresh = scoped_belief_candidates("scope-a", false, 128);
    assert_eq!(
        rows_for(&conn, &fresh),
        5,
        "stale row must be excluded when include_stale=false"
    );
}

#[test]
fn scoped_reads_clamp_to_max_candidates() {
    let conn = open_schema().expect("schema");
    for i in 0..(MAX_CANDIDATES + 10) {
        seed_belief(&conn, &format!("c{i:04}"), "scope-a", "rev-1", false).expect("seed");
    }
    let clamped = scoped_belief_candidates("scope-a", true, 10_000);
    assert_eq!(
        rows_for(&conn, &clamped),
        MAX_CANDIDATES,
        "reads must clamp to MAX_CANDIDATES"
    );
}

#[test]
fn builder_refuses_noop_advance_separator_and_bad_timestamp() {
    let base = correction("r1", "b1", "rev-1", "rev-2", "k1");

    let mut noop = base.clone();
    noop.new_revision = "rev-1".to_owned();
    assert!(
        matches!(
            build_correction_batch(&noop),
            Err(BatchRefusal::NoOpAdvance)
        ),
        "no-op advance must be refused"
    );

    let mut piped = base.clone();
    piped.idempotency_key = "a|b".to_owned();
    assert!(
        matches!(
            build_correction_batch(&piped),
            Err(BatchRefusal::KeySeparator)
        ),
        "'|' in key must be refused"
    );

    let mut sloppy_time = base.clone();
    sloppy_time.accepted_at = "2026-01-01T00:00:00Z".to_owned();
    assert!(
        matches!(
            build_correction_batch(&sloppy_time),
            Err(BatchRefusal::NonCanonicalTimestamp)
        ),
        "non-canonical timestamp must be refused"
    );

    // The canonical form itself must build.
    assert!(build_correction_batch(&base).is_ok());
}
