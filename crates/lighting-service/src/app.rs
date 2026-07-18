use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use crate::{project_routes, routes, source_routes, state::AppState};

/// Maximum accepted request body size for Source creation (1 MiB).
const SOURCE_CREATE_BODY_LIMIT: usize = 1_048_576;

/// Builds the full Axum router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    let source_routes = Router::new()
        .route(
            "/api/v1/sources",
            axum::routing::post(source_routes::create_source),
        )
        .route(
            "/api/v1/sources/{source_id}",
            axum::routing::get(source_routes::get_source),
        )
        .layer(RequestBodyLimitLayer::new(SOURCE_CREATE_BODY_LIMIT));

    let project_routes = Router::new()
        .route(
            "/api/v1/projects",
            axum::routing::post(project_routes::create_project),
        )
        .route(
            "/api/v1/projects/{project_id}",
            axum::routing::get(project_routes::get_project),
        );

    Router::new()
        .route("/health/live", axum::routing::get(routes::live))
        .route("/health/ready", axum::routing::get(routes::ready))
        .route("/api/v1/version", axum::routing::get(routes::version))
        .merge(source_routes)
        .merge(project_routes)
        .with_state(state)
}
