use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use lighting_service::{AppState, build_router};
use serde_json::Value;
use tower::ServiceExt;

fn test_state() -> AppState {
    AppState::new_unready()
}

#[tokio::test]
async fn live_route_reports_alive() {
    let app = build_router(test_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
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

    assert_eq!(json["alive"], true);
}

#[tokio::test]
async fn ready_route_reports_not_ready() {
    let app = build_router(test_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
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

    assert_eq!(json["ready"], false);
    assert_eq!(json["reason"], "storage is not configured");
}

#[tokio::test]
async fn version_route_reports_service_project_and_version() {
    let app = build_router(test_state());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/version")
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
