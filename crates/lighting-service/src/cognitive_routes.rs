use axum::{Json, extract::State, http::StatusCode};

use crate::{
    cognitive_dto::CognitiveRequest,
    cognitive_ops::{CognitiveOperationError, CognitivePlaneService},
    state::AppState,
};

pub async fn search_and_interpret(
    State(state): State<AppState>,
    Json(request): Json<CognitiveRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let (Some(source_service), Some(episode_service), Some(dreamer_service)) = (
        state.source_service,
        state.episode_service,
        state.dreamer_service,
    ) else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "cognitive plane dependencies are not configured",
        );
    };
    let service = CognitivePlaneService::new(source_service, episode_service, dreamer_service);
    match service
        .search_and_interpret(request.query, request.task, request.max_results)
        .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(serde_json::to_value(response).expect("cognitive response is serializable")),
        ),
        Err(error) => error_response(error_status(&error), &error.to_string()),
    }
}

fn error_response(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "code": "cognitive_plane_unavailable",
            "message": message,
            "canonical_mutation": false,
            "authority_changed": false
        })),
    )
}

fn error_status(error: &CognitiveOperationError) -> StatusCode {
    match error {
        CognitiveOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
        CognitiveOperationError::Tavily(TavilyError::InvalidQuery(_)) => StatusCode::BAD_REQUEST,
        CognitiveOperationError::Tavily(_)
        | CognitiveOperationError::Dreamer(DreamerOperationError::NotConfigured)
        | CognitiveOperationError::Dreamer(DreamerOperationError::Request(_)) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        CognitiveOperationError::Dreamer(_) => StatusCode::BAD_GATEWAY,
        CognitiveOperationError::Source(_) | CognitiveOperationError::Episode(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        CognitiveOperationError::Encoding(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

use crate::{dreamer_ops::DreamerOperationError, tavily::TavilyError};
