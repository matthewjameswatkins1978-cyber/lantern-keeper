//! HTTP handlers for the Marker API.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use lighting_core::MarkerId;

use crate::marker_dto::{CreateMarkerRequest, LookupQuery};
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

/// `POST /api/v1/markers`
pub async fn create_marker(
    State(state): State<AppState>,
    Json(body): Json<CreateMarkerRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.marker_service {
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

    match service.create_marker(body.text).await {
        Ok((response, is_new)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("MarkerResponse serialization must not fail");
            let status = if is_new {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            (status, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                "invalid_marker" => error_response(StatusCode::BAD_REQUEST, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}

/// `GET /api/v1/markers/{marker_id}`
pub async fn get_marker(
    State(state): State<AppState>,
    Path(marker_id_raw): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.marker_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    let marker_id = match MarkerId::new(&marker_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_marker_id".to_owned(),
                    message: "Marker ID must not be empty".to_owned(),
                },
            );
        }
    };

    match service.get_marker(&marker_id).await {
        Ok(Some(response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("MarkerResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            ApiError {
                code: "marker_not_found".to_owned(),
                message: "No Marker exists with the given ID".to_owned(),
            },
        ),
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}

/// `GET /api/v1/markers/lookup?text=<remembered phrase>`
pub async fn lookup_marker(
    State(state): State<AppState>,
    Query(query): Query<LookupQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.marker_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    if query.text.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_lookup".to_owned(),
                message: "Lookup text must not be blank".to_owned(),
            },
        );
    }

    match service.lookup_marker(&query.text).await {
        Ok(Some(response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("MarkerResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            ApiError {
                code: "marker_not_found".to_owned(),
                message: "No Marker matches the given lookup phrase".to_owned(),
            },
        ),
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                "invalid_marker" => error_response(StatusCode::BAD_REQUEST, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}
