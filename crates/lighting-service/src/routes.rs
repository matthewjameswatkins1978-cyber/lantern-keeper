use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    service: &'static str,
    status: &'static str,
    database: &'static str,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    service: &'static str,
    status: &'static str,
    error: String,
}

#[derive(Serialize)]
pub struct VersionResponse {
    service: &'static str,
    project: &'static str,
    version: &'static str,
}

pub async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<ErrorResponse>)> {
    state.store.health_check().await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                service: "Lighting",
                status: "error",
                error: error.to_string(),
            }),
        )
    })?;

    Ok(Json(HealthResponse {
        service: "Lighting",
        status: "ok",
        database: "connected",
    }))
}

pub async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: "Lighting",
        project: "Lantern Keeper",
        version: env!("CARGO_PKG_VERSION"),
    })
}
