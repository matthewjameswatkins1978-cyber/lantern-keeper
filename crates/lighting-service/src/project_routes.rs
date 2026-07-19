//! HTTP handlers for the Project API.

use axum::{extract::State, http::StatusCode, Json};

use lighting_core::{ProjectId, SourceContent, SourceKind};

use crate::project_dto::{AddFileRequest, CreateProjectRequest, RecordResultRequest};
use crate::source_dto::ApiError;
use crate::source_dto::CreateSourceResponse;
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

pub async fn record_result(
    State(state): State<AppState>,
    axum::extract::Path(project_id_raw): axum::extract::Path<String>,
    Json(body): Json<RecordResultRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let project_service = match &state.project_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let source_service = match &state.source_service {
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

    if let Err(e) = body.validate() {
        return error_response(StatusCode::BAD_REQUEST, e);
    }

    match project_service.get_project(&project_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                ApiError {
                    code: "project_not_found".to_owned(),
                    message: "No Project exists with the given ID".to_owned(),
                },
            )
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            return match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            };
        }
    }

    // Convert kind string to SourceKind
    let kind = match body.kind.as_str() {
        "markdown" => SourceKind::Markdown,
        "plain_text" => SourceKind::PlainText,
        other => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_result".to_owned(),
                    message: format!("unsupported kind: {other}"),
                },
            )
        }
    };

    // Store the content as a canonical Source (uses fingerprint dedup).
    let create_result = match source_service
        .create_source(body.title.clone(), kind, body.content.clone())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let api_error: ApiError = e.into();
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error);
        }
    };

    // Extract source_id from CreateSourceResponse (enum with Stored/Duplicate variants)
    let source_id_str = match &create_result {
        CreateSourceResponse::Stored { source_id, .. } => source_id.clone(),
        CreateSourceResponse::Duplicate { source_id } => source_id.clone(),
    };

    let source_id = match lighting_core::SourceId::parse(&source_id_str) {
        Some(id) => id,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError::internal_error(),
            )
        }
    };

    let source_content = match SourceContent::new(&body.content) {
        Ok(c) => c,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_result".to_owned(),
                    message: "result content must not be empty".to_owned(),
                },
            )
        }
    };

    match project_service
        .record_result(&project_id, body.title, source_id, source_content)
        .await
    {
        Ok(response) => {
            let status = if response.outcome == "already_recorded" {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("RecordResultResponse serialization must not fail");
            (status, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "project_not_found" => error_response(StatusCode::NOT_FOUND, api_error),
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}

pub async fn add_file(
    State(state): State<AppState>,
    axum::extract::Path(project_id_raw): axum::extract::Path<String>,
    Json(body): Json<AddFileRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let project_service = match &state.project_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let source_service = match &state.source_service {
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

    if let Err(e) = body.validate() {
        return error_response(StatusCode::BAD_REQUEST, e);
    }

    // Verify Project exists
    match project_service.get_project(&project_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                ApiError {
                    code: "project_not_found".to_owned(),
                    message: "No Project exists with the given ID".to_owned(),
                },
            )
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            return match api_error.code.as_str() {
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            };
        }
    }

    // Convert kind string to SourceKind
    let kind = match body.kind.as_str() {
        "markdown" => SourceKind::Markdown,
        "plain_text" => SourceKind::PlainText,
        other => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_add_file".to_owned(),
                    message: format!("unsupported kind: {other}"),
                },
            )
        }
    };

    // Store the content as a Source using the same capture path as source-add
    let create_result = match source_service
        .create_source(body.title.clone(), kind, body.content.clone())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let api_error: ApiError = e.into();
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error);
        }
    };

    // Extract source_id and provenance from CreateSourceResponse
    let (source_id_str, previous_source_id, capture_outcome) = match &create_result {
        CreateSourceResponse::Stored {
            source_id,
            previous_source_id,
        } => (source_id.clone(), previous_source_id.clone(), "stored"),
        CreateSourceResponse::Duplicate { source_id } => (source_id.clone(), None, "duplicate"),
    };

    let source_id = match lighting_core::SourceId::parse(&source_id_str) {
        Some(id) => id,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError::internal_error(),
            )
        }
    };

    let source_content = match SourceContent::new(&body.content) {
        Ok(c) => c,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_add_file".to_owned(),
                    message: "file content must not be empty".to_owned(),
                },
            )
        }
    };

    match project_service
        .add_file(&project_id, body.title, kind, source_id, source_content)
        .await
    {
        Ok(mut response) => {
            // Enrich with Source provenance.
            // - "linked" (first-add): override with capture outcome (stored/duplicate)
            // - "unchanged" (same source, same link): map to "duplicate"
            // - revision outcomes: preserve as-is
            match response.outcome.as_str() {
                "linked" => {
                    response.outcome = capture_outcome.to_owned();
                }
                "unchanged" => {
                    response.outcome = "duplicate".to_owned();
                }
                _ => { /* revision_captured_existing_project_link — keep as-is */ }
            }
            response.previous_source_id = previous_source_id;
            let status = if response.link_status == "already_linked" {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("AddFileResponse serialization must not fail");
            (status, Json(body))
        }
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "project_not_found" => error_response(StatusCode::NOT_FOUND, api_error),
                "storage_unavailable" => error_response(StatusCode::SERVICE_UNAVAILABLE, api_error),
                _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, api_error),
            }
        }
    }
}
