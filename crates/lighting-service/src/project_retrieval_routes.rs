//! HTTP handler for project-scoped retrieval.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::project_retrieval_dto::ProjectRetrievalRequest;
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

/// `POST /api/v1/retrieval/projects`
pub async fn retrieve_by_project(
    State(state): State<AppState>,
    Json(body): Json<ProjectRetrievalRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.project_retrieval_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    if body.project_id.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_project_id".to_owned(),
                message: "Project ID must not be empty".to_owned(),
            },
        );
    }

    match service.retrieve(&body.project_id).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("ProjectRetrievalResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = match api_error.code.as_str() {
                "storage_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "project_not_found" => StatusCode::NOT_FOUND,
                "source_unavailable" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, api_error)
        }
    }
}
