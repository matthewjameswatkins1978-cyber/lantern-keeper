use axum::http::{HeaderName, HeaderValue};
use axum::{Router, response::Redirect};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    authority_routes, cognitive_routes, dreamer_routes, episode_association_routes, episode_routes,
    epistemic_routes, ledger_routes, marker_retrieval_routes, marker_routes, memory_routes,
    project_retrieval_routes, project_routes, routes, source_routes, state::AppState,
    tethers_routes,
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
    let authority_routes = Router::new()
        .route(
            "/api/v1/authority/grants",
            axum::routing::get(authority_routes::list_grants).post(authority_routes::grant),
        )
        .route(
            "/api/v1/authority/revocations",
            axum::routing::get(authority_routes::list_revocations).post(authority_routes::revoke),
        )
        .route(
            "/api/v1/authority/check",
            axum::routing::post(authority_routes::check),
        )
        .route(
            "/api/v1/tethers/authority/check",
            axum::routing::post(authority_routes::check_tethers),
        )
        .route(
            "/api/v1/tethers/receipts",
            axum::routing::post(authority_routes::ingest_tethers_receipt),
        )
        .route(
            "/api/v1/authority/explain/{grant_id}",
            axum::routing::get(authority_routes::explain),
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
        .merge(authority_routes)
        .route(
            "/api/v1/dreamer/propose",
            axum::routing::post(dreamer_routes::propose),
        )
        .route(
            "/api/v1/cognitive/search-interpret",
            axum::routing::post(cognitive_routes::search_and_interpret),
        )
        .merge(
            Router::new()
                .route(
                    "/api/v1/claims",
                    axum::routing::post(epistemic_routes::capture_claim)
                        .get(epistemic_routes::list_claims),
                )
                .route(
                    "/api/v1/claims/live",
                    axum::routing::post(epistemic_routes::capture_live_claim),
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
                    "/api/v1/foreman/queue",
                    axum::routing::get(epistemic_routes::foreman_queue),
                )
                .route(
                    "/api/v1/foreman/{proposal_id}/review",
                    axum::routing::post(epistemic_routes::foreman_review),
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

/// Builds the intentionally small public-demo surface. The full application
/// router is not mounted here, so authority administration, memory mutation,
/// receipt ingestion, Tethers control, and execution-adjacent routes do not
/// exist in public-demo mode.
pub fn build_public_router(state: AppState) -> Router {
    Router::new()
        .route("/", axum::routing::get(|| async { Redirect::temporary("/console") }))
        .route("/health", axum::routing::get(routes::public_health))
        .route("/health/live", axum::routing::get(routes::live))
        .route("/health/ready", axum::routing::get(routes::ready))
        .route("/api/v1/version", axum::routing::get(routes::version))
        .layer(RequestBodyLimitLayer::new(16 * 1024))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn public_router_does_not_register_control_or_memory_routes() {
        let app = build_public_router(AppState::new_unready());
        for path in [
            "/api/v1/authority/grants",
            "/api/v1/authority/revocations",
            "/api/v1/tethers/authority/check",
            "/api/v1/tethers/receipts",
            "/api/v1/sources",
            "/api/v1/memories",
            "/api/v1/ledger/events",
            "/api/v1/cognitive/search-interpret",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::NOT_FOUND,
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn public_router_sets_restrictive_headers_and_health_shape() {
        let app = build_public_router(AppState::new_unready());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );

        let oversized = build_public_router(AppState::new_unready())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/health")
                    .header("content-length", 16 * 1024 + 1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            oversized.status(),
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
