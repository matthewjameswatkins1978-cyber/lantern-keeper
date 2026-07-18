use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use lighting_service::{build_router, AppState};
use lighting_store_surreal::{StoreConfig, SurrealStore};
use serde_json::Value;
use tower::ServiceExt;

async fn test_state() -> AppState {
    if std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref() == Ok("1") {
        panic!("route tests were called despite LIGHTING_SKIP_INTEGRATION_TESTS=1");
    }

    let mut config = StoreConfig::from_env();
    config.database = format!("lighting_test_routes_{}", uuid::Uuid::new_v4().simple());
    let store = SurrealStore::connect(&config)
        .await
        .unwrap_or_else(|error| panic!("failed to connect to local SurrealDB for route tests. Start it with `docker compose up -d surrealdb`. Error: {error}"));
    store
        .initialise_schema()
        .await
        .expect("schema initialisation should succeed for route tests");

    AppState::new(store)
}

#[tokio::test]
async fn health_route_reports_database_connection() {
    if std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref() == Ok("1") {
        return;
    }

    let app = build_router(test_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_route_reports_service_project_and_version() {
    if std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref() == Ok("1") {
        return;
    }

    let app = build_router(test_state().await);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/version")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json: Value = serde_json::from_slice(&body).expect("response should be JSON");

    assert_eq!(json["service"], "Lighting");
    assert_eq!(json["project"], "Lantern Keeper");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
}
