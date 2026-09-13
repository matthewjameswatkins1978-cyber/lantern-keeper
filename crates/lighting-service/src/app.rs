use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    episode_association_routes, episode_routes, epistemic_routes, ledger_routes,
    marker_retrieval_routes, marker_routes, memory_routes, project_retrieval_routes,
    project_routes, routes, source_routes, state::AppState, tethers_routes,
};

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
            "/api/v1/sources/outline",
            axum::routing::get(source_routes::get_source_outline),
        )
        .route(
            "/api/v1/sources/history",
            axum::routing::get(source_routes::get_source_history),
        )
        .route(
            "/api/v1/sources/{source_id}",
            axum::routing::get(source_routes::get_source),
        )
        .layer(RequestBodyLimitLayer::new(SOURCE_CREATE_BODY_LIMIT));

    let project_routes = Router::new()
        .route(
            "/api/v1/projects",
            axum::routing::post(project_routes::create_project).get(project_routes::list_projects),
        )
        .route(
            "/api/v1/projects/{project_id}",
            axum::routing::get(project_routes::show_project),
        );
    let project_routes = project_routes
        .route(
            "/api/v1/projects/{project_id}/record-result",
            axum::routing::post(project_routes::record_result),
        )
        .route(
            "/api/v1/projects/{project_id}/add-file",
            axum::routing::post(project_routes::add_file),
        )
        .route(
            "/api/v1/projects/{project_id}/tethers/preview",
            axum::routing::post(tethers_routes::preview),
        );

    let memory_routes = Router::new()
        .route(
            "/api/v1/memories",
            axum::routing::post(memory_routes::remember),
        )
        .route(
            "/api/v1/memories/recall",
            axum::routing::post(memory_routes::recall),
        )
        .route(
            "/api/v1/memories/context",
            axum::routing::post(memory_routes::context),
        )
        .route(
            "/api/v1/memories/{memory_id}/supersede",
            axum::routing::post(memory_routes::supersede),
        );
    let ledger_routes = Router::new().route(
        "/api/v1/ledger/events",
        axum::routing::post(ledger_routes::ingest),
    );

    let marker_routes = Router::new()
        .route(
            "/api/v1/markers",
            axum::routing::post(marker_routes::create_marker),
        )
        .route(
            "/api/v1/markers/lookup",
            axum::routing::get(marker_routes::lookup_marker),
        )
        .route(
            "/api/v1/markers/{marker_id}",
            axum::routing::get(marker_routes::get_marker),
        );

    Router::new()
        .route("/health/live", axum::routing::get(routes::live))
        .route("/health/ready", axum::routing::get(routes::ready))
        .route("/api/v1/version", axum::routing::get(routes::version))
        .merge(source_routes)
        .merge(project_routes)
        .merge(marker_routes)
        .merge(memory_routes)
        .merge(ledger_routes)
        .merge(
            Router::new()
                .route(
                    "/api/v1/claims",
                    axum::routing::post(epistemic_routes::capture_claim)
                        .get(epistemic_routes::list_claims),
                )
                .route(
                    "/api/v1/corrections",
                    axum::routing::post(epistemic_routes::record_correction),
                )
                .route(
                    "/api/v1/epistemic/context",
                    axum::routing::post(epistemic_routes::compile_context),
                )
                .route(
                    "/api/v1/claims/{claim_id}/reconcile",
                    axum::routing::post(epistemic_routes::reconcile_claim),
                )
                .route(
                    "/api/v1/beliefs",
                    axum::routing::post(epistemic_routes::create_belief)
                        .get(epistemic_routes::list_beliefs),
                )
                .route(
                    "/api/v1/beliefs/search",
                    axum::routing::get(epistemic_routes::search_beliefs),
                )
                .route(
                    "/api/v1/beliefs/stale",
                    axum::routing::get(epistemic_routes::list_stale_beliefs),
                )
                .route(
                    "/api/v1/beliefs/{belief_id}",
                    axum::routing::get(epistemic_routes::get_belief),
                )
                .route(
                    "/api/v1/beliefs/{belief_id}/history",
                    axum::routing::get(epistemic_routes::belief_history),
                )
                .route(
                    "/api/v1/beliefs/{belief_id}/explain",
                    axum::routing::get(epistemic_routes::explain_belief),
                )
                .route(
                    "/api/v1/beliefs/{belief_id}/stale",
                    axum::routing::post(epistemic_routes::mark_belief_stale),
                )
                .route(
                    "/api/v1/memory-items",
                    axum::routing::post(epistemic_routes::remember_soft),
                )
                .route(
                    "/api/v1/memory-items/search",
                    axum::routing::post(epistemic_routes::search_soft),
                )
                .route(
                    "/api/v1/relations",
                    axum::routing::post(epistemic_routes::store_relation)
                        .get(epistemic_routes::list_unresolved_relations),
                )
                .route(
                    "/api/v1/predicates",
                    axum::routing::post(epistemic_routes::create_predicate_definition)
                        .get(epistemic_routes::list_predicate_definitions),
                )
                .route(
                    "/api/v1/predicates/{key}",
                    axum::routing::get(epistemic_routes::get_predicate_definition),
                )
                .route(
                    "/api/v1/dimensions",
                    axum::routing::post(epistemic_routes::create_dimension_definition)
                        .get(epistemic_routes::list_dimension_definitions),
                )
                .route(
                    "/api/v1/dimensions/{key}",
                    axum::routing::get(epistemic_routes::get_dimension_definition),
                ),
        )
        .merge(
            Router::new()
                .route(
                    "/api/v1/episodes",
                    axum::routing::post(episode_routes::create_episode),
                )
                .route(
                    "/api/v1/episodes/{episode_id}",
                    axum::routing::get(episode_routes::get_episode),
                ),
        )
        .merge(
            Router::new()
                .route(
                    "/api/v1/episodes/{episode_id}/projects",
                    axum::routing::post(episode_association_routes::link_project)
                        .get(episode_association_routes::list_project_links),
                )
                .route(
                    "/api/v1/episodes/{episode_id}/markers",
                    axum::routing::post(episode_association_routes::link_marker)
                        .get(episode_association_routes::list_marker_links),
                ),
        )
        .route(
            "/api/v1/retrieval/markers",
            axum::routing::post(marker_retrieval_routes::retrieve_by_marker),
        )
        .route(
            "/api/v1/retrieval/projects",
            axum::routing::post(project_retrieval_routes::retrieve_by_project),
        )
        .with_state(state)
}
