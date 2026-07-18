use axum::Router;

use crate::{routes, state::AppState};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", axum::routing::get(routes::health))
        .route("/version", axum::routing::get(routes::version))
        .with_state(state)
}
