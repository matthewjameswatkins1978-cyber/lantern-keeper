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
}

/// Response returned by `GET /health/ready`.
#[derive(Serialize)]
pub struct ReadinessResponse {
    pub ready: bool,
    pub reason: String,
}

/// Small public-demo health response. It deliberately contains no endpoint,
/// storage, environment, or credential details.
#[derive(Serialize)]
pub struct PublicHealthResponse {
    pub product: &'static str,
    pub status: &'static str,
    pub mode: &'static str,
    pub version: &'static str,
    pub live_cognition: bool,
}

/// Liveness probe. Always returns 200 — reports that the process is alive.
pub async fn live() -> Json<LivenessResponse> {
    Json(LivenessResponse { alive: true })
}

/// Public-demo health. Provider availability is intentionally not inferred
/// from credentials here; the public build is replay-only.
pub async fn public_health(
    State(state): State<AppState>,
) -> (StatusCode, Json<PublicHealthResponse>) {
    let status = if state.is_ready() {
        "ready"
    } else {
        "starting"
    };
    (
        if state.is_ready() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(PublicHealthResponse {
            product: "Lantern Warden",
            status,
            mode: "public-demo",
            version: env!("CARGO_PKG_VERSION"),
            live_cognition: false,
        }),
    )
}

/// Service version information.
pub async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: "Lighting",
        project: "Lantern Keeper",
        version: env!("CARGO_PKG_VERSION"),
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
