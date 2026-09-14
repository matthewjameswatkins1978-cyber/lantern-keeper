//! HTTP handlers for Episode association endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::episode_association_dto::{LinkMarkerRequest, LinkProjectRequest};
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

/// `POST /api/v1/episodes/{episode_id}/projects`
pub async fn link_project(
    State(state): State<AppState>,
    Path(episode_id): Path<String>,
    Json(body): Json<LinkProjectRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.association_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    if let Err(e) = body.validate() {
        return error_response(StatusCode::BAD_REQUEST, e);
    }

    match service
        .link_project(&episode_id, &body.project_id, &body.kind)
        .await
    {
        Ok(()) => (StatusCode::NO_CONTENT, Json(serde_json::Value::Null)),
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "invalid_project_id" | "invalid_link_kind" => StatusCode::BAD_REQUEST,
                "episode_not_found" | "project_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}

/// `GET /api/v1/episodes/{episode_id}/projects`
pub async fn list_project_links(
    State(state): State<AppState>,
    Path(episode_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.association_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    match service.list_project_links(&episode_id).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body =
                serde_json::to_value(&response).expect("response serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "episode_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}

/// `POST /api/v1/episodes/{episode_id}/markers`
pub async fn link_marker(
    State(state): State<AppState>,
    Path(episode_id): Path<String>,
    Json(body): Json<LinkMarkerRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.association_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    if let Err(e) = body.validate() {
        return error_response(StatusCode::BAD_REQUEST, e);
    }

    match service.link_marker(&episode_id, &body.marker_id).await {
        Ok(()) => (StatusCode::NO_CONTENT, Json(serde_json::Value::Null)),
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "invalid_marker_id" => StatusCode::BAD_REQUEST,
                "episode_not_found" | "marker_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}

/// `GET /api/v1/episodes/{episode_id}/markers`
pub async fn list_marker_links(
    State(state): State<AppState>,
    Path(episode_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.association_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    match service.list_marker_links(&episode_id).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body =
                serde_json::to_value(&response).expect("response serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "episode_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}
