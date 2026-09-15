use axum::{Json, extract::State, http::StatusCode};

use crate::{
    dreamer_dto::{DreamerRequest, DreamerResponse},
    dreamer_ops::DreamerOperationError,
    state::AppState,
};

pub async fn propose(
    State(state): State<AppState>,
    Json(request): Json<DreamerRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(service) = state.dreamer_service else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            DreamerOperationError::NotConfigured,
        );
    };
    match service.propose(request.context).await {
        Ok(candidate) => (
            StatusCode::OK,
            Json(
                serde_json::to_value(DreamerResponse {
                    candidate,
                    canonical_mutation: false,
                })
                .expect("Dreamer response is serializable"),
            ),
        ),
        Err(error) => error_response(error_status(&error), error),
    }
}

fn error_response(
    status: StatusCode,
    error: DreamerOperationError,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"code": "dreamer_unavailable", "message": error.to_string()})),
    )
}

fn error_status(error: &DreamerOperationError) -> StatusCode {
    match error {
        DreamerOperationError::NotConfigured | DreamerOperationError::Request(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        DreamerOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
        DreamerOperationError::InvalidResponse(_) => StatusCode::BAD_GATEWAY,
    }
}
