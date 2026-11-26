//! LK-Q17 hosted capture proofs: ordinary remember is a note, never a claim.
//!
//! Run against real SQLite (the engine D1 exposes). The rusqlite harness
//! emulates D1 `batch()` atomicity with explicit ROLLBACK on failure.

use lighting_store_d1::{
    BatchRefusal, REMEMBER_ATTR_SCHEMA_SQL, REMEMBER_SCHEMA_SQL, RememberInput, SCHEMA_SQL,
    SqlStatement, SqlValue, build_remember_batch, idempotency_composite_key, lookup_remember,
    memory_note_list, source_bytes,
};
use rusqlite::{Connection, Result as SqlResult};

const CANONICAL_T0: &str = "2026-01-01T00:00:00.000000000Z";

fn open_schema() -> SqlResult<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_ATTR_SCHEMA_SQL)?;
    Ok(conn)
}

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
        Err(e) => {
            conn.execute_batch("ROLLBACK")?;
            return Err(e);
        }
    }
    Ok(())
}

fn remember(id: &str, content: &str, caller_key: &str) -> RememberInput {
    RememberInput {
        source_id: format!("src-{id}"),
        memory_id: format!("mem-{id}"),
        principal: "pi".to_owned(),
        scope_hash: "scope-a".to_owned(),
        content: content.to_owned(),
        fingerprint: format!("fp-{content}"),
        payload_hash: format!("hash-{caller_key}"),
        actor: "pi".to_owned(),
        receipt_id: format!("receipt-{id}"),
        accepted_at: CANONICAL_T0.to_owned(),
        result_json: format!("{{\"result\":\"{id}\"}}"),
        idempotency_key: caller_key.to_owned(),
    }
}

fn batch_of(write: &RememberInput) -> Vec<SqlStatement> {
    build_remember_batch(write).expect("test remember must build")
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
    let mut prepared = conn.prepare(&statement.sql).expect("prepare");
    let mut rows = prepared
        .query(rusqlite::params_from_iter(params))
        .expect("query");
    let mut out = Vec::new();
    while let Some(row) = rows.next().expect("row") {
        let id: String = row.get(0).expect("col0");
        let content: String = row.get(1).expect("col1");
        out.push(vec![id, content]);
    }
    out
}

#[test]
fn capture_then_exact_read_with_byte_equality() {
    let conn = open_schema().expect("schema");
    let content = "caf\u{E9} \u{1F3EE} exact bytes \r\n end";
    let write = remember("r1", content, "k1");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    // Exact read returns byte-identical content.
    let rows = rows_for(&conn, &source_bytes("src-r1", "scope-a"));
    assert_eq!(rows.len(), 1, "seeded source must read back");
    assert_eq!(rows[0][1], content, "bytes must survive verbatim");

    // A note is NEVER a claim.
    assert_eq!(count(&conn, "epistemic_claim"), 0, "no claim row may exist");
    assert_eq!(count(&conn, "belief"), 0, "no belief row may exist");
    assert_eq!(count(&conn, "belief_revision"), 0);
    assert_eq!(count(&conn, "memory_item"), 1);
    assert_eq!(count(&conn, "remember_idempotency"), 1);
}

#[test]
fn replay_returns_original_without_duplicating() {
    let conn = open_schema().expect("schema");
    let write = remember("r1", "hello", "k-replay");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    // Same key + same payload: the batch aborts on the receipt PK (fail
    // closed), and the worker answers from lookup_remember. Here we prove
    // the storage half: nothing duplicates.
    let retry = remember("r1-dup", "hello", "k-replay");
    assert!(
        run_batch(&conn, &batch_of(&retry)).is_err(),
        "replay must not re-execute"
    );
    assert_eq!(count(&conn, "memory_item"), 1);
    assert_eq!(count(&conn, "source"), 1);

    // Lookup finds the original receipt.
    let key = idempotency_composite_key("pi", "scope-a", "k-replay");
    let found: Option<String> = {
        let mut prepared = conn.prepare(&lookup_remember(&key).sql).expect("prepare");
        let mut rows = prepared.query(rusqlite::params![key]).expect("query");
        rows.next()
            .expect("row")
            .map(|row| row.get(2).expect("result_json"))
    };
    assert_eq!(found, Some("{\"result\":\"r1\"}".to_owned()));

    // Changed payload under the same key conflicts.
    let mut conflict = remember("r1-x", "different", "k-replay");
    conflict.payload_hash = "hash-other".to_owned();
    assert!(
        run_batch(&conn, &batch_of(&conflict)).is_err(),
        "changed payload must conflict"
    );
}

#[test]
fn persisted_hash_matches_replay_comparison() {
    // Regression for the Q17 qualification blocker: the worker persisted
    // content_hash but compared the envelope hash, so every replay
    // conflicted. The contract is: the stored payload_hash MUST equal the
    // value the worker compares. This test runs the full cycle at the seam:
    // batch -> persisted row -> lookup -> compare.
    let conn = open_schema().expect("schema");
    let write = remember("r1", "hello", "k-replay");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    let stored: String = conn
        .query_row(
            "SELECT payload_hash FROM remember_idempotency WHERE key = 'pi|scope-a|k-replay'",
            [],
            |r| r.get(0),
        )
        .expect("receipt row");
    // The worker compares this exact stored value against its freshly
    // computed hash; equality is what makes replay return the receipt.
    let key = idempotency_composite_key("pi", "scope-a", "k-replay");
    let found: Option<(String, String)> = {
        let mut prepared = conn.prepare(&lookup_remember(&key).sql).expect("prepare");
        let mut rows = prepared.query(rusqlite::params![key]).expect("query");
        rows.next()
            .expect("row")
            .map(|row| (row.get(0).expect("key"), row.get(1).expect("payload_hash")))
    };
    let (found_key, found_hash) = found.expect("receipt must be found");
    assert_eq!(found_key, "pi|scope-a|k-replay");
    assert_eq!(
        found_hash, stored,
        "compared value must equal persisted value"
    );
    assert_eq!(
        stored, "hash-k-replay",
        "fixture hash passes through untouched"
    );
}

#[test]
fn fingerprint_is_content_addressed_not_write_addressed() {
    // Regression for the d91bd09 follow-up: the envelope digest was reused
    // as the source fingerprint, so the SAME bytes remembered under two
    // keys got different fingerprints. Fingerprint must depend on content
    // alone; the replay digest carries the write identity.
    let conn = open_schema().expect("schema");
    let mut first = remember("r1", "same bytes", "k-one");
    first.source_id = "src-same".to_owned();
    run_batch(&conn, &batch_of(&first)).expect("first accepted");
    let mut second = remember("r2", "same bytes", "k-two");
    second.source_id = "src-same".to_owned();
    second.memory_id = "mem-r2".to_owned();
    second.receipt_id = "receipt-r2".to_owned();
    run_batch(&conn, &batch_of(&second)).expect("second accepted");

    // One source row (ON CONFLICT DO NOTHING), one fingerprint.
    assert_eq!(count(&conn, "source"), 1);
    let fp: String = conn
        .query_row(
            "SELECT fingerprint FROM source WHERE id = 'src-same'",
            [],
            |r| r.get(0),
        )
        .expect("fingerprint");
    assert_eq!(fp, "fp-same bytes", "fingerprint follows content only");
    // But two distinct keyed writes.
    assert_eq!(count(&conn, "memory_item"), 2);
    assert_eq!(count(&conn, "remember_idempotency"), 2);
}

#[test]
fn denied_scope_sees_nothing() {
    let conn = open_schema().expect("schema");
    let write = remember("r1", "secret", "k1");
    run_batch(&conn, &batch_of(&write)).expect("accepted");

    // Wrong scope: the join finds no owning note -> invisible.
    let rows = rows_for(&conn, &source_bytes("src-r1", "scope-b"));
    assert!(rows.is_empty(), "denied scope must see nothing");
    let listed = rows_for(&conn, &memory_note_list("scope-b", 32));
    assert!(listed.is_empty());
}

#[test]
fn builder_refuses_blank_separator_and_bad_timestamp() {
    let base = remember("r1", "content", "k1");

    let mut blank = base.clone();
    blank.content = "   ".to_owned();
    assert!(matches!(
        build_remember_batch(&blank),
        Err(BatchRefusal::BlankContent)
    ));

    let mut piped = base.clone();
    piped.idempotency_key = "a|b".to_owned();
    assert!(matches!(
        build_remember_batch(&piped),
        Err(BatchRefusal::KeySeparator)
    ));

    let mut sloppy = base.clone();
    sloppy.accepted_at = "2026-01-01T00:00:00Z".to_owned();
    assert!(matches!(
        build_remember_batch(&sloppy),
        Err(BatchRefusal::NonCanonicalTimestamp)
    ));

    assert!(build_remember_batch(&base).is_ok());
}

#[test]
fn attribution_is_preserved_on_the_note_row() {
    // Q26 attributed_capture: who captured this must survive on the row,
    // not only in the idempotency receipt.
    let conn = open_schema().expect("schema");
    let write = remember("r1", "attributed note", "k1");
    run_batch(&conn, &batch_of(&write)).expect("accepted");
    let actor: String = conn
        .query_row(
            "SELECT actor FROM memory_item WHERE id = 'mem-r1'",
            [],
            |r| r.get(0),
        )
        .expect("actor");
    assert_eq!(actor, "pi");
}

#[test]
fn note_list_is_bounded_and_scoped() {
    let conn = open_schema().expect("schema");
    for i in 0..10 {
        let write = remember(&format!("r{i}"), &format!("note {i}"), &format!("k{i}"));
        run_batch(&conn, &batch_of(&write)).expect("accepted");
    }
    // Other scope is invisible.
    let mut other = remember("rx", "other", "kx");
    other.scope_hash = "scope-b".to_owned();
    other.source_id = "src-rx".to_owned();
    other.memory_id = "mem-rx".to_owned();
    run_batch(&conn, &batch_of(&other)).expect("accepted");

    let listed = rows_for(&conn, &memory_note_list("scope-a", 5));
    assert_eq!(listed.len(), 5, "limit must bound the list");
    let all = rows_for(&conn, &memory_note_list("scope-a", 128));
    assert_eq!(all.len(), 10, "scope-b note must not leak in");

    // limit 0 -> no rows.
    let zero = rows_for(&conn, &memory_note_list("scope-a", 0));
    assert!(zero.is_empty());
}

#[test]
fn remember_writes_no_claim_evidence() {
    // Belt and braces on the product rule: after any accepted remember,
    // every claim/belief/revision/receipt table that belongs to the
    // correction path stays empty.
    let conn = open_schema().expect("schema");
    let write = remember("r1", "plain note", "k1");
    run_batch(&conn, &batch_of(&write)).expect("accepted");
    for table in [
        "epistemic_claim",
        "belief",
        "belief_revision",
        "correction_idempotency",
        "mutation_receipt",
    ] {
        assert_eq!(count(&conn, table), 0, "{table} must stay empty");
    }
}
