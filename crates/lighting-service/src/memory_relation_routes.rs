use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::{
    memory_dto::MemoryRelationRequest, memory_ops::MemoryOperationError, source_dto::ApiError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct RelationQuery {
    pub memory_id: Option<String>,
}

fn failure(error: MemoryOperationError) -> (StatusCode, Json<serde_json::Value>) {
    let api: ApiError = error.into();
    let status = if api.code == "invalid_memory" {
        StatusCode::BAD_REQUEST
    } else if api.code == "storage_unavailable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(serde_json::to_value(api).expect("API errors serialize")),
    )
}

pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<MemoryRelationRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return failure(MemoryOperationError::Unavailable);
    };
    match service.remember_relation(request).await {
        Ok(relation) => (
            StatusCode::OK,
            Json(serde_json::to_value(relation).expect("relation serializes")),
        ),
        Err(error) => failure(error),
    }
}

pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<RelationQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = &state.memory_service else {
        return failure(MemoryOperationError::Unavailable);
    };
    match service.relations(query.memory_id.as_deref()).await {
        Ok(relations) => (
            StatusCode::OK,
            Json(serde_json::json!({"relations": relations})),
        ),
        Err(error) => failure(error),
    }
}
