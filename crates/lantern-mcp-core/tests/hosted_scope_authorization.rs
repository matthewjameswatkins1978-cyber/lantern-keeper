//! Q18-HOSTED — denied scope sees nothing, at the authorization layer.
//!
//! The counterpart of the read-layer proof in
//! `lighting-store-d1/tests/hosted_correction_proof.rs`: before any
//! statement is built, `authorize` must refuse a scope the credential does
//! not hold, and refuse write tools on read-only credentials. Real code,
//! no live calls.

use lantern_mcp_core::{
    CoreError, Principal, Tool, authorize, load_credentials, parse_correct, sha256_hex,
};
use std::collections::BTreeMap;

const TOKEN: &str = "q18-hosted-test-token";

fn scope(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn principal_with(scopes: &[&str], permissions: &[&str]) -> Principal {
    let scope_values: Vec<serde_json::Value> = scopes
        .iter()
        .map(|scope| serde_json::from_str(scope).expect("scope object"))
        .collect();
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let file = serde_json::json!({
        "credentials": [{
            "token_sha256": sha256_hex(TOKEN),
            "principal": "pi",
            "scopes": scope_values,
            "permissions": permissions
        }]
    });
    let entries = load_credentials(&file.to_string()).expect("credentials load");
    lantern_mcp_core::authenticate(&entries, TOKEN).expect("authenticate")
}

/// The granted scope: normalised equality, exact match, no prefix widening.
#[test]
fn granted_scope_is_authorised_for_reads_and_writes() {
    let principal = principal_with(&[r#"{"project":"q18h"}"#], &["read", "write"]);
    authorize(
        &principal,
        Tool::Search,
        Some(&scope(&[("project", "q18h")])),
    )
    .expect("granted scope must pass");
    authorize(
        &principal,
        Tool::Correct,
        Some(&scope(&[("project", "q18h")])),
    )
    .expect("granted scope must pass writes too");
}

/// Denied scope sees nothing: refused before any statement is built.
#[test]
fn denied_scope_is_forbidden_before_any_data_access() {
    let principal = principal_with(&[r#"{"project":"q18h"}"#], &["read", "write"]);
    let outcome = authorize(
        &principal,
        Tool::Correct,
        Some(&scope(&[("project", "other")])),
    );
    assert!(
        matches!(outcome, Err(CoreError::Forbidden(_))),
        "an ungranted scope must be Forbidden: {outcome:?}"
    );
    let read_outcome = authorize(
        &principal,
        Tool::Search,
        Some(&scope(&[("project", "other")])),
    );
    assert!(
        matches!(read_outcome, Err(CoreError::Forbidden(_))),
        "reads under a denied scope must be Forbidden too: {read_outcome:?}"
    );
}

#[test]
fn workshop_steward_is_bound_to_project_only_scopes() {
    let settings = serde_json::json!({"credentials": [{
        "token_sha256": sha256_hex(TOKEN), "principal": "configured-steward",
        "scopes": [], "workshop_steward": true, "permissions": ["read", "write"]
    }]});
    let entries = load_credentials(&settings.to_string()).expect("load steward");
    let principal = lantern_mcp_core::authenticate(&entries, TOKEN).expect("authenticate");
    for tool in [Tool::Search, Tool::Remember] {
        assert!(authorize(&principal, tool, Some(&scope(&[("project", "unseen")]))).is_ok());
        assert!(matches!(
            authorize(
                &principal,
                tool,
                Some(&scope(&[("project", "unseen"), ("tenant", "foreign")]))
            ),
            Err(CoreError::Forbidden(_))
        ));
    }
}

/// No scope argument at all is refused at parse time, so a caller cannot
/// skip the allowlist check.
#[test]
fn scoped_tool_arguments_require_a_scope() {
    let params = serde_json::json!({
        "belief_id": "b1",
        "expected_revision": "t0",
        "new_value": "v",
        "idempotency_key": "k1"
    });
    assert!(parse_correct(&params).is_err(), "missing scope must fail");
}

/// Read-only credentials cannot write.
#[test]
fn read_only_credential_cannot_correct() {
    let principal = principal_with(&[r#"{"project":"q18h"}"#], &["read"]);
    let outcome = authorize(
        &principal,
        Tool::Correct,
        Some(&scope(&[("project", "q18h")])),
    );
    assert!(
        matches!(outcome, Err(CoreError::Forbidden(_))),
        "read-only credential must be refused the correct tool: {outcome:?}"
    );
}

/// Correct argument hygiene: no composite separator, bounded key length.
#[test]
fn correct_arguments_reject_separator_and_oversized_keys() {
    let base = serde_json::json!({
        "scope": {"project": "q18h"},
        "belief_id": "b1",
        "expected_revision": "t0",
        "new_value": "v"
    });

    let mut with_pipe = base.clone();
    with_pipe["idempotency_key"] = serde_json::json!("bad|key");
    assert!(parse_correct(&with_pipe).is_err(), "'|' must be refused");

    let mut oversized = base.clone();
    oversized["idempotency_key"] = serde_json::json!("k".repeat(129));
    assert!(parse_correct(&oversized).is_err(), "oversized key refused");

    let mut ok = base;
    ok["idempotency_key"] = serde_json::json!("fine-key");
    assert!(parse_correct(&ok).is_ok(), "valid arguments pass");
}
