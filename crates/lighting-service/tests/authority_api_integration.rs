use std::collections::BTreeMap;

use axum::{body::Body, http::Request};
use chrono::{DateTime, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityGrant, AuthorityGrantId, AuthorityRequest, AuthorityScope, EpisodeId,
    PrincipalId,
};
use lighting_service::{AppState, AuthorityService, build_router};
use tower::ServiceExt;

fn grant() -> AuthorityGrant {
    AuthorityGrant {
        id: AuthorityGrantId::new("grant-demo").unwrap(),
        issuer_principal_id: PrincipalId::new("human:demo").unwrap(),
        delegate_principal_id: PrincipalId::new("agent:lucy").unwrap(),
        capability_id: "demo.export_summary".to_owned(),
        capability_version: "1".to_owned(),
        scope: AuthorityScope::from_pairs([
            ("project", "demo-project"),
            ("destination", "approved-demo-sink"),
        ])
        .unwrap(),
        issued_at: at("2026-09-14T10:00:00Z"),
        expires_at: Some(at("2026-09-15T00:00:00Z")),
        source_episode_id: EpisodeId::new("episode-demo").unwrap(),
        authenticated_session_id: "control-session".to_owned(),
        constraints: BTreeMap::new(),
        created_at: at("2026-09-14T10:00:00Z"),
    }
}

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn state(service: AuthorityService) -> AppState {
    let mut state = AppState::new_unready();
    state.authority_service = Some(service);
    state
}

#[tokio::test]
async fn control_plane_requires_session_and_csrf_and_check_is_deterministic() {
    let service = AuthorityService::new();
    let session = service.open_control_session().await;
    let app = build_router(state(service));
    let session_id = session.session_id.as_str();
    let csrf = session.csrf_token.as_str();

    let grant_body = serde_json::json!({"grant": grant()});
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/authority/grants")
                .header("content-type", "application/json")
                .body(Body::from(grant_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/authority/grants")
                .header("content-type", "application/json")
                .header("x-lantern-session", session_id)
                .header("x-lantern-csrf", csrf)
                .body(Body::from(grant_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 201);

    let check_body = serde_json::json!({
        "check": AuthorityCheck {
            request: AuthorityRequest {
                action_id: "action-1".to_owned(),
                principal_id: PrincipalId::new("agent:lucy").unwrap(),
                capability_id: "demo.export_summary".to_owned(),
                capability_version: "1".to_owned(),
                scope: AuthorityScope::from_pairs([
                    ("project", "demo-project"),
                    ("destination", "approved-demo-sink"),
                ]).unwrap(),
                constraints: BTreeMap::new(),
            },
            at: at("2026-09-14T12:00:00Z"),
        }
    });
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/authority/check")
                .header("content-type", "application/json")
                .body(Body::from(check_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let allowed: serde_json::Value = body_json(response).await;
    assert_eq!(allowed["decision"], "ALLOW");
    assert_eq!(allowed["grant_id"], "grant-demo");

    let denied = serde_json::json!({
        "check": AuthorityCheck {
            request: AuthorityRequest {
                action_id: "action-2".to_owned(),
                principal_id: PrincipalId::new("agent:lucy").unwrap(),
                capability_id: "demo.export_summary".to_owned(),
                capability_version: "1".to_owned(),
                scope: AuthorityScope::from_pairs([
                    ("project", "demo-project"),
                    ("destination", "attacker.invalid"),
                ]).unwrap(),
                constraints: BTreeMap::new(),
            },
            at: at("2026-09-14T12:00:00Z"),
        }
    });
    let response = app
        .oneshot(
            Request::post("/api/v1/authority/check")
                .header("content-type", "application/json")
                .body(Body::from(denied.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let denied: serde_json::Value = body_json(response).await;
    assert_eq!(denied["decision"], "DENY");
    assert_eq!(denied["reason_code"], "SCOPE_MISMATCH");
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
