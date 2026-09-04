use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    memory_dto::{ContextRequest, RecallRequest, RememberRequest},
    memory_ops::MemoryOperationError,
    state::AppState,
};

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

pub async fn remember(
    State(state): State<AppState>,
    Json(request): Json<RememberRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.memory_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            MemoryOperationError::Unavailable,
        );
    };
    match service.remember(request).await {
        Ok(memory) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"memory": memory})),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn recall(
    State(state): State<AppState>,
    Json(request): Json<RecallRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.memory_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            MemoryOperationError::Unavailable,
        );
    };
    match service
        .recall(request.project_id, request.phrase, request.include_inactive)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn context(
    State(state): State<AppState>,
    Json(request): Json<ContextRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.memory_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            MemoryOperationError::Unavailable,
        );
    };
    match service.context(request.project_id, request.query).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))),
        Err(error) => error_response(error_status(&error), error),
    }
}

pub async fn supersede(
    Path(memory_id): Path<String>,
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.memory_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            MemoryOperationError::Unavailable,
        );
    };
    match service.supersede(&memory_id).await {
        Ok(response) => (StatusCode::OK, Json(serde_json::json!(response))),
        Err(error) => error_response(error_status(&error), error),
    }
}

fn error_status(error: &MemoryOperationError) -> StatusCode {
    match error {
        MemoryOperationError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        MemoryOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
        MemoryOperationError::NotFound => StatusCode::NOT_FOUND,
        MemoryOperationError::Repository(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
