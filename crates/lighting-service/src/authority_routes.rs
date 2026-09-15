use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use lighting_core::{AuthorityCheck, AuthorityDecision, DenyReason};

use crate::{
    authority_dto::{AuthorityCheckRequest, GrantIntent, RevocationIntent},
    authority_ops::AuthorityOperationError,
    state::AppState,
};

pub async fn list_grants(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    match service.list_grants().await {
        Ok(grants) => (StatusCode::OK, Json(serde_json::json!({"grants": grants}))),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn list_revocations(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    match service.list_revocations().await {
        Ok(revocations) => (
            StatusCode::OK,
            Json(serde_json::json!({"revocations": revocations})),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn check(
    State(state): State<AppState>,
    Json(request): Json<AuthorityCheckRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    match service.check(&request.check).await {
        Ok(decision) => (
            StatusCode::OK,
            Json(decision_json(&request.check, decision)),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn explain(
    Path(grant_id): Path<String>,
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    let Ok(grant_id) = lighting_core::AuthorityGrantId::new(grant_id) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            AuthorityOperationError::Invalid("grant_id cannot be blank".to_owned()),
        );
    };
    let explained = match service.explain(&grant_id).await {
        Ok(explained) => explained,
        Err(error) => return error_response(error_status(&error), error),
    };
    let Some((grant, revocation)) = explained else {
        return error_response(
            StatusCode::NOT_FOUND,
            AuthorityOperationError::Invalid("grant not found".to_owned()),
        );
    };
    let active = revocation.is_none()
        && grant
            .expires_at
            .is_none_or(|expires_at| expires_at > Utc::now());
    (
        StatusCode::OK,
        Json(serde_json::json!({"grant": grant, "revocation": revocation, "active": active})),
    )
}

pub async fn grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<GrantIntent>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    let Ok(Json(intent)) = body else {
        return error_response(
            StatusCode::BAD_REQUEST,
            AuthorityOperationError::Invalid("invalid grant intent".to_owned()),
        );
    };
    let result = service
        .issue_from_control_plane(
            &header(&headers, "x-lantern-session"),
            &header(&headers, "x-lantern-csrf"),
            intent,
        )
        .await;
    match result {
        Ok(grant) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"status": "granted", "grant": grant})),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<RevocationIntent>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.authority_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            AuthorityOperationError::Unavailable,
        );
    };
    let Ok(Json(intent)) = body else {
        return error_response(
            StatusCode::BAD_REQUEST,
            AuthorityOperationError::Invalid("invalid revocation intent".to_owned()),
        );
    };
    let result = service
        .revoke_from_control_plane(
            &header(&headers, "x-lantern-session"),
            &header(&headers, "x-lantern-csrf"),
            intent,
        )
        .await;
    match result {
        Ok(revocation) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"status": "revoked", "revocation": revocation})),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

fn header(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn decision_json(check: &AuthorityCheck, decision: AuthorityDecision) -> serde_json::Value {
    match decision {
        AuthorityDecision::Allow {
            grant_id,
            expires_at,
        } => serde_json::json!({
            "decision": "ALLOW",
            "grant_id": grant_id,
            "expires_at": expires_at,
            "action_id": check.request.action_id,
        }),
        AuthorityDecision::Deny { reason } => serde_json::json!({
            "decision": "DENY",
            "reason_code": reason_code(&reason),
            "action_id": check.request.action_id,
        }),
    }
}

fn reason_code(reason: &DenyReason) -> &'static str {
    match reason {
        DenyReason::NoMatchingGrant => "NO_MATCHING_GRANT",
        DenyReason::ExpiredGrant => "EXPIRED_GRANT",
        DenyReason::RevokedGrant => "REVOKED_GRANT",
        DenyReason::PrincipalMismatch => "PRINCIPAL_MISMATCH",
        DenyReason::CapabilityMismatch => "CAPABILITY_MISMATCH",
        DenyReason::ScopeMismatch => "SCOPE_MISMATCH",
        DenyReason::ConstraintMismatch => "CONSTRAINT_MISMATCH",
    }
}

fn error_response(
    status: StatusCode,
    error: AuthorityOperationError,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"code": "authority_error", "message": error.to_string()})),
    )
}

fn error_status(error: &AuthorityOperationError) -> StatusCode {
    match error {
        AuthorityOperationError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        AuthorityOperationError::Unauthenticated | AuthorityOperationError::InvalidCsrf => {
            StatusCode::UNAUTHORIZED
        }
        AuthorityOperationError::ExpiredSession => StatusCode::UNAUTHORIZED,
        AuthorityOperationError::Persistence(_) => StatusCode::SERVICE_UNAVAILABLE,
        AuthorityOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
    }
}
