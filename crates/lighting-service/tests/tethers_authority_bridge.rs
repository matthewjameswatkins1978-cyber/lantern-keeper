use axum::{Router, body::Body, http::Request};
use chrono::{Duration, Utc};
use lighting_service::{AppState, AuthorityService, build_router};
use tower::ServiceExt;

fn app(service: AuthorityService) -> Router {
    let mut state = AppState::new_unready();
    state.authority_service = Some(service);
    build_router(state)
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post(
    app: &Router,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> axum::response::Response {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn bridge_check_uses_server_clock_and_exact_scope() {
    let app = app(AuthorityService::new().with_tethers_audit_token("test-token"));
    let response = post(
        &app,
        "/api/v1/tethers/authority/check",
        None,
        serde_json::json!({
            "wire_version": "lantern.authority.check/1",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "scope": {"kind": "unrestricted"},
            "constraints": {}
        }),
    )
    .await;
    assert_eq!(response.status(), 401);
    let response = post(
        &app,
        "/api/v1/tethers/authority/check",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.authority.check/1",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "scope": {"kind": "unrestricted"},
            "constraints": {}
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(json(response).await["decision"], "DENY");
}

#[tokio::test]
async fn receipt_intent_cannot_supply_server_owned_fields() {
    let app = app(AuthorityService::new().with_tethers_audit_token("test-token"));
    let response = post(
        &app,
        "/api/v1/tethers/receipts",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.receipt.intent/1",
            "kind": "decision",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "decision": "DENY",
            "scope": {"kind": "unrestricted"},
            "executed": false,
            "receipt_hash": "sha256:attacker"
        }),
    )
    .await;
    assert_eq!(response.status(), 400);

    let response = post(
        &app,
        "/api/v1/tethers/receipts",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.receipt.intent/1",
            "kind": "decision",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "decision": "DENY",
            "scope": {"kind": "unrestricted"},
            "executed": false
        }),
    )
    .await;
    assert_eq!(response.status(), 201);
    let receipt = json(response).await["receipt"].clone();
    assert!(
        receipt["receipt_id"]
            .as_str()
            .unwrap()
            .starts_with("receipt_")
    );
    assert!(
        receipt["receipt_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(receipt["previous_receipt_hash"].is_null());
    assert!(receipt["requested_at"].as_str().is_some());
    assert!(receipt["decided_at"].as_str().is_some());
}

#[tokio::test]
async fn valid_grant_allows_and_receipts_are_hash_linked() {
    let service = AuthorityService::new().with_tethers_audit_token("test-token");
    let session = service
        .open_control_session(lighting_core::PrincipalId::new("human:matthew").unwrap())
        .await;
    let grant = service
        .issue_from_control_plane(
            &session.session_id,
            &session.csrf_token,
            lighting_service::authority_dto::GrantIntent {
                delegate_principal_id: lighting_core::PrincipalId::new("agent:lucy").unwrap(),
                capability_id: "demo.export_summary".to_owned(),
                capability_version: "1".to_owned(),
                scope: lighting_core::AuthorityScope::from_pairs([("kind", "unrestricted")])
                    .unwrap(),
                expires_at: Some(Utc::now() + Duration::hours(1)),
                constraints: Default::default(),
            },
        )
        .await
        .unwrap();
    let app = app(service);
    let check = post(
        &app,
        "/api/v1/tethers/authority/check",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.authority.check/1",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "scope": {"kind": "unrestricted"},
            "constraints": {}
        }),
    )
    .await;
    let checked = json(check).await;
    assert_eq!(checked["decision"], "ALLOW");
    assert_eq!(checked["grant_id"], grant.id.to_string());

    let decision = post(
        &app,
        "/api/v1/tethers/receipts",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.receipt.intent/1",
            "kind": "decision",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "decision": "ALLOW",
            "grant_id": grant.id,
            "scope": {"kind": "unrestricted"},
            "executed": false
        }),
    )
    .await;
    assert_eq!(decision.status(), 201);
    let decision_receipt = json(decision).await["receipt"].clone();

    let outcome = post(
        &app,
        "/api/v1/tethers/receipts",
        Some("test-token"),
        serde_json::json!({
            "wire_version": "lantern.receipt.intent/1",
            "kind": "outcome",
            "action_id": "action-1",
            "principal_id": "agent:lucy",
            "capability_id": "demo.export_summary",
            "capability_version": "1",
            "decision": "ALLOW",
            "grant_id": grant.id,
            "scope": {"kind": "unrestricted"},
            "executed": true,
            "outcome": "SUCCESS",
            "result_ref": "execution-1"
        }),
    )
    .await;
    assert_eq!(outcome.status(), 201);
    let outcome_receipt = json(outcome).await["receipt"].clone();
    assert_eq!(
        outcome_receipt["previous_receipt_hash"],
        decision_receipt["receipt_hash"]
    );
    assert_ne!(
        outcome_receipt["receipt_hash"],
        decision_receipt["receipt_hash"]
    );
}
