use axum::{Json, extract::State, http::StatusCode};

use crate::{
    ledger_ops::LedgerOperationError,
    state::AppState,
};

pub async fn ingest(
    State(state): State<AppState>,
    Json(event): Json<lighting_core::LedgerEvent>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.ledger_service else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, LedgerOperationError::Unavailable);
    };
    match service.ingest(event).await {
        Ok(response) => {
            let status = if response.duplicate { StatusCode::OK } else { StatusCode::CREATED };
            (status, Json(serde_json::json!(response)))
        }
        Err(error) => error_response(StatusCode::BAD_REQUEST, error),
    }
}

fn error_response(
    status: StatusCode,
    error: impl Into<crate::source_dto::ApiError>,
) -> (StatusCode, Json<serde_json::Value>) {
    let error = error.into();
    (
        status,
        Json(serde_json::json!({"code": error.code, "message": error.message})),
    )
}
