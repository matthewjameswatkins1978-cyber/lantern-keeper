use std::collections::BTreeMap;

use axum::{Router, body::Body, http::Request, response::Response};
use chrono::{Duration, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityGrantId, AuthorityRequest, AuthorityScope, PrincipalId,
};
use lighting_service::{AppState, AuthorityService, ControlSession, build_router};
use tower::ServiceExt;

fn state(service: AuthorityService) -> AppState {
    let mut state = AppState::new_unready();
    state.authority_service = Some(service);
    state
}

fn principal(value: &str) -> PrincipalId {
    PrincipalId::new(value).unwrap()
}

fn grant_intent() -> serde_json::Value {
    serde_json::json!({
        "delegate_principal_id": "agent:lucy",
        "capability_id": "demo.export_summary",
        "capability_version": "1",
        "scope": {
            "project": "demo-project",
            "destination": "approved-demo-sink"
        },
        "expires_at": (Utc::now() + Duration::hours(1)).to_rfc3339(),
        "constraints": {}
    })
}

fn check_body(destination: &str) -> serde_json::Value {
    serde_json::json!({
        "check": AuthorityCheck {
            request: AuthorityRequest {
                action_id: "action-1".to_owned(),
                principal_id: principal("agent:lucy"),
                capability_id: "demo.export_summary".to_owned(),
                capability_version: "1".to_owned(),
                scope: AuthorityScope::from_pairs([
                    ("project", "demo-project"),
                    ("destination", destination),
                ]).unwrap(),
                constraints: BTreeMap::new(),
            },
            at: Utc::now() + Duration::seconds(1),
        }
    })
}

async fn post_json(
    app: &Router,
    path: &str,
    session: Option<&ControlSession>,
    body: serde_json::Value,
) -> Response {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(session) = session {
        request = request
            .header("x-lantern-session", &session.session_id)
            .header("x-lantern-csrf", &session.csrf_token);
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn control_plane_requires_session_and_csrf_and_check_is_deterministic() {
    let service = AuthorityService::new();
    let session = service.open_control_session(principal("human:alice")).await;
    let app = build_router(state(service));

    let response = post_json(&app, "/api/v1/authority/grants", None, grant_intent()).await;
    assert_eq!(response.status(), 401);

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/authority/grants")
                .header("content-type", "application/json")
                .header("x-lantern-session", &session.session_id)
                .header("x-lantern-csrf", "wrong-csrf")
                .body(Body::from(grant_intent().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    let response = post_json(
        &app,
        "/api/v1/authority/grants",
        Some(&session),
        grant_intent(),
    )
    .await;
    assert_eq!(response.status(), 201);
    let created = body_json(response).await;
    let grant_id = created["grant"]["id"].clone();

    let response = post_json(
        &app,
        "/api/v1/authority/check",
        None,
        check_body("approved-demo-sink"),
    )
    .await;
    assert_eq!(response.status(), 200);
    let allowed = body_json(response).await;
    assert_eq!(allowed["decision"], "ALLOW");
    assert_eq!(allowed["grant_id"], grant_id);

    let response = post_json(
        &app,
        "/api/v1/authority/check",
        None,
        check_body("attacker.invalid"),
    )
    .await;
    let denied = body_json(response).await;
    assert_eq!(denied["decision"], "DENY");
    assert_eq!(denied["reason_code"], "SCOPE_MISMATCH");
}

#[tokio::test]
async fn session_principal_cannot_be_spoofed_by_grant_body() {
    let service = AuthorityService::new();
    let session = service.open_control_session(principal("human:alice")).await;
    let app = build_router(state(service.clone()));
    let mut body = grant_intent();
    body["issuer_principal_id"] = serde_json::json!("human:bob");

    let response = post_json(&app, "/api/v1/authority/grants", Some(&session), body).await;
    assert_eq!(response.status(), 400);
    assert!(service.list_grants().await.unwrap().is_empty());
}

#[tokio::test]
async fn session_id_in_grant_is_server_owned() {
    let service = AuthorityService::new();
    let session = service.open_control_session(principal("human:alice")).await;
    let app = build_router(state(service));

    let response = post_json(
        &app,
        "/api/v1/authority/grants",
        Some(&session),
        grant_intent(),
    )
    .await;
    assert_eq!(response.status(), 201);
    let created = body_json(response).await;
    assert_eq!(created["grant"]["issuer_principal_id"], "human:alice");
    assert_eq!(
        created["grant"]["authenticated_session_id"],
        session.session_id
    );
    assert!(
        created["grant"]["id"]
            .as_str()
            .unwrap()
            .starts_with("grant_")
    );
    assert!(
        created["grant"]["source_episode_id"]
            .as_str()
            .unwrap()
            .starts_with("authority-control-")
    );
    assert_eq!(
        created["grant"]["issued_at"],
        created["grant"]["created_at"]
    );
}

#[tokio::test]
async fn different_principal_cannot_revoke_grant() {
    let service = AuthorityService::new();
    let alice = service.open_control_session(principal("human:alice")).await;
    let bob = service.open_control_session(principal("human:bob")).await;
    let app = build_router(state(service.clone()));

    let created = body_json(
        post_json(
            &app,
            "/api/v1/authority/grants",
            Some(&alice),
            grant_intent(),
        )
        .await,
    )
    .await;
    let grant_id: AuthorityGrantId =
        serde_json::from_value(created["grant"]["id"].clone()).unwrap();

    let response = post_json(
        &app,
        "/api/v1/authority/revocations",
        Some(&bob),
        serde_json::json!({"grant_id": grant_id}),
    )
    .await;
    assert_eq!(response.status(), 400);
    assert!(service.list_revocations().await.unwrap().is_empty());
}

#[tokio::test]
async fn revocation_session_id_is_server_owned() {
    let service = AuthorityService::new();
    let session = service.open_control_session(principal("human:alice")).await;
    let app = build_router(state(service));

    let created = body_json(
        post_json(
            &app,
            "/api/v1/authority/grants",
            Some(&session),
            grant_intent(),
        )
        .await,
    )
    .await;
    let grant_id = created["grant"]["id"].clone();
    let response = post_json(
        &app,
        "/api/v1/authority/revocations",
        Some(&session),
        serde_json::json!({"grant_id": grant_id}),
    )
    .await;
    assert_eq!(response.status(), 201);
    let revoked = body_json(response).await;
    assert_eq!(revoked["revocation"]["issuer_principal_id"], "human:alice");
    assert_eq!(
        revoked["revocation"]["authenticated_session_id"],
        session.session_id
    );
    assert!(
        revoked["revocation"]["id"]
            .as_str()
            .unwrap()
            .starts_with("revocation_")
    );
    assert!(
        revoked["revocation"]["source_episode_id"]
            .as_str()
            .unwrap()
            .starts_with("authority-control-")
    );
}

#[tokio::test]
async fn arbitrary_source_episode_cannot_be_supplied() {
    let service = AuthorityService::new();
    let session = service.open_control_session(principal("human:alice")).await;
    let app = build_router(state(service.clone()));
    let mut body = grant_intent();
    body["source_episode_id"] = serde_json::json!("episode-attacker-controlled");

    let response = post_json(&app, "/api/v1/authority/grants", Some(&session), body).await;
    assert_eq!(response.status(), 400);
    assert!(service.list_grants().await.unwrap().is_empty());
}

#[tokio::test]
async fn expired_control_session_cannot_mutate_authority() {
    let service = AuthorityService::new();
    let session = service
        .open_control_session_with_expiry(
            principal("human:alice"),
            Utc::now() - Duration::seconds(1),
        )
        .await;
    let app = build_router(state(service.clone()));

    let response = post_json(
        &app,
        "/api/v1/authority/grants",
        Some(&session),
        grant_intent(),
    )
    .await;
    assert_eq!(response.status(), 401);
    assert!(service.list_grants().await.unwrap().is_empty());
}
