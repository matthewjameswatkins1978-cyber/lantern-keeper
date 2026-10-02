use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;

use crate::state::AppState;

/// Response returned by `GET /health/live`.
#[derive(Serialize)]
pub struct LivenessResponse {
    pub alive: bool,
}

/// Response returned by `GET /api/v1/version`.
#[derive(Serialize)]
pub struct VersionResponse {
    pub service: &'static str,
    pub project: &'static str,
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datastore_mode: Option<String>,
    pub surrealdb_expected_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surrealdb_observed_version: Option<String>,
}

/// Response returned by `GET /health/ready`.
#[derive(Serialize)]
pub struct ReadinessResponse {
    pub ready: bool,
    pub reason: String,
}

/// Liveness probe. Always returns 200 — reports that the process is alive.
pub async fn live() -> Json<LivenessResponse> {
    Json(LivenessResponse { alive: true })
}

/// Service version information.
pub async fn version(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse {
        service: "Lighting",
        project: "Lantern Keeper",
        version: env!("CARGO_PKG_VERSION"),
        datastore_mode: state.datastore_mode.clone(),
        surrealdb_expected_version: state.surrealdb_expected_version,
        surrealdb_observed_version: state.surrealdb_observed_version.clone(),
    })
}

/// Readiness probe.
///
/// Returns 200 only after the service has connected to SurrealDB and applied /
/// verified its schema migration. Storage state is reported honestly.
pub async fn ready(State(state): State<AppState>) -> (StatusCode, Json<ReadinessResponse>) {
    if state.is_ready() {
        (
            StatusCode::OK,
            Json(ReadinessResponse {
                ready: true,
                reason: "durable Source storage is ready".to_owned(),
            }),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadinessResponse {
                ready: false,
                reason: "storage is not configured".to_owned(),
            }),
        )
    }
}
