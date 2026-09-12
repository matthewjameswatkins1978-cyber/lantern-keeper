use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    epistemic_dto::{BeliefRequest, ClaimRequest, MemoryItemRequest, MemoryItemSearchRequest},
    epistemic_ops::EpistemicOperationError,
    state::AppState,
};

fn error_response(error: EpistemicOperationError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match error {
        EpistemicOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
        EpistemicOperationError::NotFound => StatusCode::NOT_FOUND,
        EpistemicOperationError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        EpistemicOperationError::Repository(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let api: crate::source_dto::ApiError = error.into();
    (
        status,
        Json(serde_json::json!({"code": api.code, "message": api.message})),
    )
}

pub async fn capture_claim(
    State(state): State<AppState>,
    Json(request): Json<ClaimRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.capture_claim(request).await {
        Ok(claim) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"claim": claim})),
        ),
        Err(error) => error_response(error),
    }
}

pub async fn create_belief(
    State(state): State<AppState>,
    Json(request): Json<BeliefRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.create_belief(request).await {
        Ok(belief) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"belief": belief})),
        ),
        Err(error) => error_response(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct BeliefListQuery {
    #[serde(default)]
    pub include_stale: bool,
}

pub async fn list_beliefs(
    State(state): State<AppState>,
    Query(query): Query<BeliefListQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.list_beliefs(query.include_stale).await {
        Ok(beliefs) => (
            StatusCode::OK,
            Json(serde_json::json!({"beliefs": beliefs})),
        ),
        Err(error) => error_response(error),
    }
}

pub async fn mark_belief_stale(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<crate::epistemic_dto::StaleBeliefRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.mark_belief_stale(&id, request.reason).await {
        Ok(belief) => (StatusCode::OK, Json(serde_json::json!({"belief": belief}))),
        Err(error) => error_response(error),
    }
}

pub async fn remember_soft(
    State(state): State<AppState>,
    Json(request): Json<MemoryItemRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.remember_soft(request).await {
        Ok(memory_item) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"memory_item": memory_item})),
        ),
        Err(error) => error_response(error),
    }
}

pub async fn search_soft(
    State(state): State<AppState>,
    Json(request): Json<MemoryItemSearchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.search_soft(request).await {
        Ok(memory_items) => (
            StatusCode::OK,
            Json(serde_json::json!({"memory_items": memory_items})),
        ),
        Err(error) => error_response(error),
    }
}

pub async fn store_relation(
    State(state): State<AppState>,
    Json(request): Json<crate::epistemic_dto::RelationRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.store_relation(request).await {
        Ok(relation) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"relation": relation})),
        ),
        Err(error) => error_response(error),
    }
}

pub async fn list_unresolved_relations(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.epistemic_service else {
        return error_response(EpistemicOperationError::Unavailable);
    };
    match service.list_unresolved_relations().await {
        Ok(relations) => (
            StatusCode::OK,
            Json(serde_json::json!({"relations": relations})),
        ),
        Err(error) => error_response(error),
    }
}
