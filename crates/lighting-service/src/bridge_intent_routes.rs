use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    bridge_intent_dto::{
        BridgeIntentResponse, ClaimBridgeIntentRequest, CompleteBridgeIntentRequest,
        ListBridgeIntentsResponse,
    },
    bridge_intent_ops::BridgeIntentOperationError,
    state::AppState,
};

fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "code": code,
            "message": message,
        })),
    )
}

pub async fn claim(
    State(state): State<AppState>,
    Json(request): Json<ClaimBridgeIntentRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.bridge_intent_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "bridge intent service is unavailable",
        );
    };

    match service.claim(request).await {
        Ok(outcome) => (StatusCode::OK, Json(serde_json::json!(outcome))),
        Err(err) => map_error(err),
    }
}

pub async fn complete(
    State(state): State<AppState>,
    Json(request): Json<CompleteBridgeIntentRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.bridge_intent_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "bridge intent service is unavailable",
        );
    };

    match service.complete(request).await {
        Ok(record) => (
            StatusCode::OK,
            Json(serde_json::json!(BridgeIntentResponse { intent: record })),
        ),
        Err(err) => map_error(err),
    }
}

pub async fn get(
    Path(intent_id): Path<String>,
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.bridge_intent_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "bridge intent service is unavailable",
        );
    };

    match service.get(&intent_id).await {
        Ok(Some(record)) => (
            StatusCode::OK,
            Json(serde_json::json!(BridgeIntentResponse { intent: record })),
        ),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "intent_not_found",
            &format!("intent {intent_id} was not found"),
        ),
        Err(err) => map_error(err),
    }
}

pub async fn list(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.bridge_intent_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "bridge intent service is unavailable",
        );
    };

    match service.list_all().await {
        Ok(records) => (
            StatusCode::OK,
            Json(serde_json::json!(ListBridgeIntentsResponse {
                intents: records
            })),
        ),
        Err(err) => map_error(err),
    }
}

fn map_error(err: BridgeIntentOperationError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        BridgeIntentOperationError::Unavailable => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "bridge intent service is unavailable",
        ),
        BridgeIntentOperationError::NotFound(id) => error_response(
            StatusCode::NOT_FOUND,
            "intent_not_found",
            &format!("intent {id} not found"),
        ),
        BridgeIntentOperationError::Repository(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &e.to_string(),
        ),
    }
}
