use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use crate::{
    memory_dto::{MemoryContextRequest, MemorySearchQuery, RememberRequest},
    memory_ops::MemoryOperationError,
    source_dto::ApiError,
    state::AppState,
};

fn error(error: ApiError, status: StatusCode) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::to_value(error).expect("API errors serialize")),
    )
}

fn operation_error(operation: MemoryOperationError) -> (StatusCode, Json<serde_json::Value>) {
    let api: ApiError = operation.into();
    let status = if api.code == "invalid_memory" {
        StatusCode::BAD_REQUEST
    } else if api.code == "storage_unavailable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    error(api, status)
}

pub async fn remember(
    State(state): State<AppState>,
    Json(body): Json<RememberRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.remember(body).await {
        Ok(value) => (
            StatusCode::OK,
            Json(serde_json::to_value(value).expect("response serializes")),
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<MemorySearchQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service
        .search(
            &query.q,
            query.scope.as_deref(),
            query.include_history.unwrap_or(false),
            query.limit.unwrap_or(20),
        )
        .await
    {
        Ok(value) => (
            StatusCode::OK,
            Json(serde_json::to_value(value).expect("response serializes")),
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn context(
    State(state): State<AppState>,
    Json(body): Json<MemoryContextRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.context(body).await {
        Ok(value) => (
            StatusCode::OK,
            Json(serde_json::to_value(value).expect("response serializes")),
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn get(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.get(&memory_id).await {
        Ok(Some(value)) => (
            StatusCode::OK,
            Json(serde_json::to_value(value).expect("response serializes")),
        ),
        Ok(None) => error(
            ApiError {
                code: "memory_not_found".to_owned(),
                message: "No memory exists with the given ID".to_owned(),
            },
            StatusCode::NOT_FOUND,
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn export(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.export().await {
        Ok(memories) => (
            StatusCode::OK,
            Json(serde_json::json!({"memories": memories})),
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn forget(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.forget(&memory_id).await {
        Ok(Some(memory)) => (
            StatusCode::OK,
            Json(serde_json::to_value(memory).expect("memory serializes")),
        ),
        Ok(None) => error(
            ApiError {
                code: "memory_not_found".to_owned(),
                message: "No memory exists with the given ID".to_owned(),
            },
            StatusCode::NOT_FOUND,
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn history(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.history(&memory_id).await {
        Ok(value) => (
            StatusCode::OK,
            Json(serde_json::json!({ "memories": value })),
        ),
        Err(e) => operation_error(e),
    }
}

pub async fn doctor(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.doctor().await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(e) => operation_error(e),
    }
}

pub async fn audit(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return error(
            ApiError::storage_unavailable(),
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    match service.audit().await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(e) => operation_error(e),
    }
}
