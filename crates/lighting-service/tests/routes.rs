use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use lighting_service::{
    build_router,
    state::{DatabaseHealth, HealthCheckFuture},
    AppState,
};
use serde_json::Value;
use tower::ServiceExt;

#[derive(Clone)]
struct FakeDatabase {
    result: Result<(), String>,
}

impl DatabaseHealth for FakeDatabase {
    fn health_check(&self) -> HealthCheckFuture<'_> {
        Box::pin(async move { self.result.clone() })
    }
}

fn test_state(result: Result<(), String>) -> AppState {
    AppState::new(FakeDatabase { result })
}

#[tokio::test]
async fn health_route_reports_database_connection() {
    let app = build_router(test_state(Ok(())));

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

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json: Value = serde_json::from_slice(&body).expect("response should be JSON");

    assert_eq!(json["service"], "Lighting");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"], "connected");
}

#[tokio::test]
async fn health_route_reports_database_failure() {
    let app = build_router(test_state(Err("database is unavailable".to_owned())));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let json: Value = serde_json::from_slice(&body).expect("response should be JSON");

    assert_eq!(json["service"], "Lighting");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"], "database is unavailable");
}

#[tokio::test]
async fn version_route_reports_service_project_and_version() {
    let app = build_router(test_state(Ok(())));

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
