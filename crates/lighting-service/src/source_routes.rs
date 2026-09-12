//! HTTP handlers for the Source API.

use axum::{extract::State, http::StatusCode, Json};

use lighting_core::{SourceId, SourceKind};

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

/// `GET /api/v1/sources/history?kind=markdown&title=d:/projects/doc.md`
///
/// Lists all revisions of a logical Source identified by (kind, title).
pub async fn get_source_history(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let kind_raw = match params.get("kind") {
        Some(k) => k.clone(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: "query parameter 'kind' is required".to_owned(),
                },
            )
        }
    };
    let title = match params.get("title") {
        Some(t) => t.clone(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: "query parameter 'title' is required".to_owned(),
                },
            )
        }
    };
    if title.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_source".to_owned(),
                message: "title must not be empty".to_owned(),
            },
        );
    }
    let service = match &state.source_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let kind = match kind_raw.as_str() {
        "markdown" => SourceKind::Markdown,
        "plain_text" => SourceKind::PlainText,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: format!("unsupported source kind: {kind_raw}"),
                },
            )
        }
    };

    match service.list_history(kind, &title).await {
        Ok(Some(response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("SourceHistoryResponse serialization must not fail");
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

/// `GET /api/v1/sources/outline?kind=markdown&title=d:/projects/doc.md`
///
/// Returns the Markdown heading outline for the current revision of a logical
/// Source.
pub async fn get_source_outline(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let kind_raw = match params.get("kind") {
        Some(k) => k.clone(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: "query parameter 'kind' is required".to_owned(),
                },
            )
        }
    };
    let title = match params.get("title") {
        Some(t) => t.clone(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: "query parameter 'title' is required".to_owned(),
                },
            )
        }
    };
    if title.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_source".to_owned(),
                message: "title must not be empty".to_owned(),
            },
        );
    }
    let service = match &state.source_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            )
        }
    };

    let kind = match kind_raw.as_str() {
        "markdown" => SourceKind::Markdown,
        "plain_text" => SourceKind::PlainText,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_source".to_owned(),
                    message: format!("unsupported source kind: {kind_raw}"),
                },
            )
        }
    };

    match service.get_outline(kind, &title).await {
        Ok(Some(response)) => {
            #[allow(clippy::expect_used)]
            let body = serde_json::to_value(&response)
                .expect("SourceOutlineResponse serialization must not fail");
            (StatusCode::OK, Json(body))
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            ApiError {
                code: "source_not_found".to_owned(),
                message: format!("No Source captured for title '{title}' with kind '{kind_raw}'"),
            },
        ),
        Err(e) => {
            let api_error: ApiError = e.into();
            match api_error.code.as_str() {
                "source_not_markdown" => {
                    error_response(StatusCode::UNPROCESSABLE_ENTITY, api_error)
                }
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
