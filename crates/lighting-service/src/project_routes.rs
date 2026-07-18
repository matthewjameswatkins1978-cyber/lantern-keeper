//! HTTP handlers for the Project API.

use axum::{extract::State, http::StatusCode, Json};

use lighting_core::ProjectId;

use crate::project_dto::CreateProjectRequest;
use crate::source_dto::ApiError;
use crate::state::AppState;

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

pub async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.project_service {
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

    match service.create_project(body.name, body.status).await {
        Ok(response) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("ProjectResponse serialization must not fail");
            (StatusCode::CREATED, Json(body))
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

pub async fn get_project(
    State(state): State<AppState>,
    axum::extract::Path(project_id_raw): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let service = match &state.project_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let project_id = match ProjectId::new(&project_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_project_id".to_owned(),
                    message: "Project ID must not be empty".to_owned(),
                },
            )
        }
    };

    match service.get_project(&project_id).await {
        Ok(Some(project_response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&project_response)
                .expect("ProjectResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            ApiError {
                code: "project_not_found".to_owned(),
                message: "No Project exists with the given ID".to_owned(),
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
