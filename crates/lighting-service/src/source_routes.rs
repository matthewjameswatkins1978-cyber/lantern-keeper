//! HTTP handlers for the Source API.

use axum::{extract::State, http::StatusCode, Json};

use lighting_core::SourceId;

use crate::{
    source_dto::{ApiError, CreateSourceRequest, CreateSourceResponse},
    state::AppState,
};

/// `POST /api/v1/sources`
pub async fn create_source(
    State(state): State<AppState>,
    Json(body): Json<CreateSourceRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.source_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let kind = match body.validate() {
        Ok(k) => k,
        Err(api_error) => return error_response(StatusCode::BAD_REQUEST, api_error),
    };

    match service.create_source(body.title, kind, body.content).await {
        Ok(response) => {
            let (status, body) = match &response {
                CreateSourceResponse::Stored { .. } => {
                    (StatusCode::CREATED, serde_json::to_value(&response))
                }
                CreateSourceResponse::Duplicate { .. } => {
                    (StatusCode::OK, serde_json::to_value(&response))
                }
            };
            #[allow(clippy::expect_used)]
            let body = body.expect("response serialization must not fail");
            (status, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}

/// `GET /api/v1/sources/{source_id}`
pub async fn get_source(
    State(state): State<AppState>,
    axum::extract::Path(source_id_raw): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.source_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let source_id = match SourceId::parse(&source_id_raw) {
        Some(id) => id,
        None => return error_response(StatusCode::BAD_REQUEST, ApiError::invalid_source_id()),
    };

    match service.get_source(&source_id).await {
        Ok(Some(source_response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&source_response)
                .expect("SourceResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Ok(None) => error_response(StatusCode::NOT_FOUND, ApiError::not_found()),
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}
