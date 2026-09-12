//! HTTP handlers for the Episode API.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use lighting_core::EpisodeId;

use crate::episode_dto::CreateEpisodeRequest;
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

/// `POST /api/v1/episodes`
pub async fn create_episode(
    State(state): State<AppState>,
    Json(body): Json<CreateEpisodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.episode_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    if let Err(e) = body.validate() {
        return error_response(StatusCode::BAD_REQUEST, e);
    }

    match service
        .create_episode(body.title, body.source_id, body.start_byte, body.end_byte)
        .await
    {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("EpisodeResponse serialization must not fail");
            (StatusCode::CREATED, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "invalid_episode" | "invalid_episode_range" => StatusCode::BAD_REQUEST,
                "source_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}

/// `GET /api/v1/episodes/{episode_id}`
pub async fn get_episode(
    State(state): State<AppState>,
    Path(episode_id_raw): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.episode_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let episode_id = match EpisodeId::new(&episode_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_episode_id".to_owned(),
                    message: "Episode ID must not be empty".to_owned(),
                },
            )
        }
    };

    match service.get_episode(&episode_id).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("EpisodeResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "source_unavailable" => StatusCode::CONFLICT,
                "episode_not_found" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}
