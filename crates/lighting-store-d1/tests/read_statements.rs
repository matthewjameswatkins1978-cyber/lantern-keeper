//! Read-statement proofs for the hosted read tools (C2): scoped, bound,
//! clamped, zero-write shapes. These test the SQL *construction*; execution
//! happens in the worker against D1.

use lighting_store_d1::{
    MAX_CANDIDATES, SqlStatement, SqlValue, belief_detail, belief_revisions, search_beliefs,
};

fn sql(statement: &SqlStatement) -> &str {
    &statement.sql
}

fn text_params(statement: &SqlStatement) -> Vec<&str> {
    statement
        .params
        .iter()
        .filter_map(|param| match param {
            SqlValue::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn search_is_scoped_bound_and_pattern_is_parameterised() {
    let statement = search_beliefs("scope-a", &["%lantern%".to_owned()], 9_999);
    let text = sql(&statement);
    assert!(
        text.contains("scope_hash = ?"),
        "search must constrain by scope index"
    );
    assert!(
        !text.contains("lantern"),
        "pattern must never be interpolated into SQL"
    );
    assert!(text.contains("payload LIKE ? COLLATE NOCASE"));
    assert!(text.ends_with("LIMIT ?"), "limit must be a bound parameter");
    // Clamp: LIMIT param is MAX_CANDIDATES even though 9_999 was requested.
    let limit = statement
        .params
        .iter()
        .find_map(|param| match param {
            SqlValue::Integer(value) => Some(*value),
            _ => None,
        })
        .expect("limit param");
    assert_eq!(limit, MAX_CANDIDATES as i64);
    // Scope and pattern are both bound as text.
    let texts = text_params(&statement);
    assert_eq!(texts[0], "scope-a");
    assert_eq!(texts[1], "%lantern%");
}

#[test]
fn search_suppresses_stale_like_scoped_candidates() {
    // GAP-1: search_beliefs used to return superseded beliefs while the
    // sibling scoped path filtered them. Both now carry `stale = 0`.
    let stmt = search_beliefs("scope-a", &["%x%".to_owned()], 64);
    let text = sql(&stmt);
    assert!(
        text.contains("stale = 0"),
        "search must suppress stale beliefs"
    );
}

#[test]
fn search_refuses_empty_patterns_and_zero_limit() {
    let no_patterns = search_beliefs("scope-a", &[], 64);
    assert!(
        sql(&no_patterns).contains("WHERE 0"),
        "empty patterns must return no rows, never an unscoped scan"
    );
    let zero_limit = search_beliefs("scope-a", &["%x%".to_owned()], 0);
    assert!(sql(&zero_limit).contains("WHERE 0"));
}

#[test]
fn search_or_chain_binds_every_pattern() {
    let statement = search_beliefs("scope-a", &["%a%".to_owned(), "%b%".to_owned()], 10);
    let text = sql(&statement);
    assert_eq!(
        text.matches("payload LIKE ? COLLATE NOCASE").count(),
        2,
        "one bound placeholder per pattern"
    );
    let texts = text_params(&statement);
    assert_eq!(texts, vec!["scope-a", "%a%", "%b%"]);
}

#[test]
fn belief_detail_is_scope_confined_point_lookup() {
    let statement = belief_detail("b1", "scope-a");
    let text = sql(&statement);
    assert!(
        text.contains("WHERE id = ? AND scope_hash = ?"),
        "point lookups must stay inside the caller's scope"
    );
    assert!(text.contains("LIMIT 1"));
    assert!(!text.contains("b1"), "ids must be bound, not interpolated");
    assert_eq!(text_params(&statement), vec!["b1", "scope-a"]);
    // Zero-write shape: a SELECT only.
    assert!(text.starts_with("SELECT "));
}

#[test]
fn revisions_are_ordered_and_clamped() {
    let statement = belief_revisions("b1", 10_000);
    let text = sql(&statement);
    assert!(text.contains("ORDER BY created_at DESC"));
    let limit = statement
        .params
        .iter()
        .find_map(|param| match param {
            SqlValue::Integer(value) => Some(*value),
            _ => None,
        })
        .expect("limit param");
    assert_eq!(limit, 64, "revision history clamps at 64");
    let low = belief_revisions("b1", 0);
    let low_limit = low
        .params
        .iter()
        .find_map(|param| match param {
            SqlValue::Integer(value) => Some(*value),
            _ => None,
        })
        .expect("limit param");
    assert_eq!(low_limit, 1, "limit 0 must not mean unbounded");
}
