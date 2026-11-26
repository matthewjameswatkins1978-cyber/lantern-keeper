//! Ordinary-note lookup proofs (LK-HOME-FINAL import discoverability).
//!
//! `memory_note_list` is a newest-first page with a hard 128-row bound, and
//! a Basic Memory import can exceed that page, so `search_memory_notes`
//! supplies the bounded scoped query that reaches older notes. Both reads
//! carry the `source_id` handle `source_bytes` resolves to exact content.
//! Run against real SQLite (the engine D1 exposes).

use lighting_store_d1::{
    MAX_CANDIDATES, REMEMBER_ATTR_SCHEMA_SQL, REMEMBER_SCHEMA_SQL, SCHEMA_SQL, SqlStatement,
    SqlValue, memory_note_list, search_memory_notes, source_bytes,
};
use rusqlite::{Connection, Result as SqlResult};

const SCOPE_A: &str = "scope-a-hash";
const SCOPE_B: &str = "scope-b-hash";

fn open_schema() -> SqlResult<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_SCHEMA_SQL)?;
    conn.execute_batch(REMEMBER_ATTR_SCHEMA_SQL)?;
    Ok(conn)
}

/// Canonical 30-char hosted timestamp for note `index` (0..150).
fn created_at(index: usize) -> String {
    format!(
        "2026-01-01T{:02}:{:02}:00.000000000Z",
        index / 60,
        index % 60
    )
}

/// One synthetic source + its scoped ordinary note (the shape an import
/// projection and a keyed remember both produce).
fn insert_note(conn: &Connection, index: usize, scope: &str, archived: bool) -> SqlResult<()> {
    let id = format!("note-{index:03}");
    let source_id = format!("src-{index:03}");
    let content = format!("imported markdown body {index:03} — café 🏮\n");
    let at = created_at(index);
    conn.execute(
        "INSERT INTO source (id, kind, title, content, fingerprint, created_at) VALUES (?1, 'markdown', ?2, ?3, ?4, ?5)",
        rusqlite::params![source_id, id, content, format!("fp-{scope}-{index:03}"), at],
    )?;
    conn.execute(
        "INSERT INTO memory_item (id, scope_hash, content, kind, salience, archived, payload, created_at, source_id, actor) VALUES (?1, ?2, ?3, 'note', 0.0, ?4, ?3, ?5, ?6, 'import:hosted-seed-sql-v1')",
        rusqlite::params![
            id,
            scope,
            content,
            i64::from(archived),
            at,
            source_id
        ],
    )?;
    Ok(())
}

/// (id, content, created_at, source_id) per row of one prepared read.
fn rows_for(conn: &Connection, statement: &SqlStatement) -> Vec<(String, String, String, String)> {
    let params: Vec<rusqlite::types::Value> = statement
        .params
        .iter()
        .map(|param| match param {
            SqlValue::Text(text) => rusqlite::types::Value::Text(text.clone()),
            SqlValue::Integer(int) => rusqlite::types::Value::Integer(*int),
            SqlValue::Real(real) => rusqlite::types::Value::Real(*real),
            SqlValue::Null => rusqlite::types::Value::Null,
        })
        .collect();
    let mut prepared = conn.prepare(&statement.sql).expect("prepare");
    let mut query = prepared
        .query(rusqlite::params_from_iter(params))
        .expect("query");
    let mut out = Vec::new();
    while let Some(row) = query.next().expect("row") {
        out.push((
            row.get::<_, String>(0).expect("id"),
            row.get::<_, String>(1).expect("content"),
            row.get::<_, String>(2).expect("created_at"),
            row.get::<_, String>(3).expect("source_id"),
        ));
    }
    out
}

fn seed_bulk(conn: &Connection) {
    for index in 0..150 {
        insert_note(conn, index, SCOPE_A, false).expect("insert note");
    }
    // One archived note must stay invisible to both reads.
    insert_note(conn, 150, SCOPE_A, true).expect("insert archived note");
}

#[test]
fn plain_list_is_bounded_and_returns_source_handles() {
    let conn = open_schema().expect("schema");
    seed_bulk(&conn);

    let rows = rows_for(&conn, &memory_note_list(SCOPE_A, 128));
    assert_eq!(
        rows.len(),
        MAX_CANDIDATES,
        "the newest page stops at the hard bound"
    );
    // Newest first: note-149 leads, and the oldest 22 fall off the page.
    assert_eq!(rows[0].0, "note-149");
    assert!(
        !rows.iter().any(|row| row.0 == "note-000"),
        "an imported note older than the newest page is unreachable by plain list"
    );
    assert!(
        !rows.iter().any(|row| row.0 == "note-150"),
        "archived notes never list"
    );
    // Every listed note carries a non-empty source_id handle.
    for row in &rows {
        assert!(row.3.starts_with("src-"), "handle on {}", row.0);
    }

    // Cross-scope: another scope sees nothing.
    assert_eq!(rows_for(&conn, &memory_note_list(SCOPE_B, 128)).len(), 0);
}

#[test]
fn scoped_query_reaches_notes_beyond_the_newest_page() {
    let conn = open_schema().expect("schema");
    seed_bulk(&conn);

    // The oldest imported note is beyond the plain page (proved above);
    // the scoped query finds it without widening the bound.
    let statement = search_memory_notes(SCOPE_A, &["%body 000%".to_owned()], 128);
    assert!(
        !statement.sql.contains("body 000"),
        "the pattern is bound as a parameter, never interpolated"
    );
    assert!(
        statement
            .params
            .contains(&SqlValue::Text("%body 000%".to_owned())),
        "pattern travels as a bound parameter"
    );
    let rows = rows_for(&conn, &statement);
    assert_eq!(rows.len(), 1, "exactly the one old note matches");
    assert_eq!(rows[0].0, "note-000");
    assert_eq!(rows[0].3, "src-000", "query results carry the handle too");
    assert_eq!(
        rows[0].2, "2026-01-01T00:00:00.000000000Z",
        "created_at preserved for ordering"
    );

    // The handle resolves through the scoped exact-bytes path.
    let get = source_bytes(&rows[0].3, SCOPE_A);
    let fetched = rows_for(&conn, &get);
    assert_eq!(fetched.len(), 1, "get resolves the imported source");
    assert_eq!(
        fetched[0].1, rows[0].1,
        "query content == exact source bytes"
    );

    // Matching still crosses the whole scope, not just the newest page:
    // a middle note below the page boundary is found too (note-010 sits in
    // the oldest 22, outside the plain 128-row listing).
    let middle = rows_for(
        &conn,
        &search_memory_notes(SCOPE_A, &["%body 010%".to_owned()], 128),
    );
    assert_eq!(middle.len(), 1);
    assert_eq!(middle[0].0, "note-010");

    // Scoped and bounded: no other scope, no archived rows, no unbounded run.
    assert_eq!(
        rows_for(
            &conn,
            &search_memory_notes(SCOPE_B, &["%body%".to_owned()], 128)
        )
        .len(),
        0
    );
    assert_eq!(
        rows_for(
            &conn,
            &search_memory_notes(SCOPE_A, &["%body 150%".to_owned()], 128)
        )
        .len(),
        0,
        "archived notes stay invisible to the query as well"
    );
    assert_eq!(
        rows_for(&conn, &search_memory_notes(SCOPE_A, &[], 128)).len(),
        0,
        "empty patterns refuse the unscoped scan"
    );
    let all = rows_for(
        &conn,
        &search_memory_notes(SCOPE_A, &["%".to_owned()], 10_000),
    );
    assert_eq!(
        all.len(),
        MAX_CANDIDATES,
        "limit above the hard bound is clamped, never widened"
    );
}
