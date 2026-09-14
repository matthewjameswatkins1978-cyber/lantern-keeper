//! HTTP handler for marker-led retrieval.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::marker_retrieval_dto::MarkerRetrievalRequest;
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

/// `POST /api/v1/retrieval/markers`
pub async fn retrieve_by_marker(
    State(state): State<AppState>,
    Json(body): Json<MarkerRetrievalRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.retrieval_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    if body.text.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_retrieval".to_owned(),
                message: "Retrieval text must not be blank".to_owned(),
            },
        );
    }

    match service.retrieve(&body.text).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("MarkerRetrievalResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}
