//! HTTP route for the Tethers preview endpoint.
//!
//! POST /api/v1/projects/{project_id}/tethers/preview

use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use lighting_core::ProjectId;

use crate::source_dto::ApiError;
use crate::state::AppState;
use crate::tethers_engine_client::{TethersEngineClient, TethersEngineError};
use crate::tethers_preview::{build_preview_request, PreviewInput, TethersResponse};

// ── Request DTO ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TethersPreviewRequest {
    pub task: String,
    pub changed_files: u64,
    pub evaluation_id: String,
    pub event_id: String,
}

// ── Handler ───────────────────────────────────────────────────────

pub async fn preview(
    State(state): State<AppState>,
    Path(project_id_raw): Path<String>,
    Json(body): Json<TethersPreviewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. Validate project through ProjectService.
    let project_id = match ProjectId::new(&project_id_raw) {
        Ok(id) => id,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "invalid_project_id".into(),
                    message: "Project ID must not be empty".into(),
                },
            );
        }
    };

    let project_service = match &state.project_service {
        Some(svc) => svc,
        None => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::storage_unavailable(),
            );
        }
    };

    match project_service.show(&project_id).await {
        Err(e) => {
            let api_error: ApiError = e.into();
            let status = if api_error.code == "project_not_found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return error_response(status, api_error);
        }
        Ok(_project) => {
            // Project exists — continue to Tethers evaluation.
        }
    }

    // 2. Build preview input.
    let input = PreviewInput {
        evaluation_id: body.evaluation_id,
        event_id: body.event_id,
        project_id: project_id_raw,
        task: body.task,
        changed_files: body.changed_files,
    };

    let request = build_preview_request(&input);

    // 3. Obtain engine client from state (test seam) or from_env.
    let client = if let Some(ref client) = state.tethers_client {
        client.clone()
    } else if let Some((status_code, ref code, ref message)) = state.tethers_env_error {
        return error_response(
            StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            ApiError {
                code: code.clone(),
                message: message.clone(),
            },
        );
    } else {
        match TethersEngineClient::from_env() {
            Ok(c) => c,
            Err(TethersEngineError::MissingEnginePath) => {
                return error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ApiError {
                        code: "tethers_unavailable".into(),
                        message: "Tethers engine is not configured".into(),
                    },
                );
            }
            Err(e) => {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    ApiError {
                        code: "tethers_engine_error".into(),
                        message: format!("{e}"),
                    },
                );
            }
        }
    };

    // 4. Evaluate.
    let response: TethersResponse = match client.evaluate(&request).await {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                ApiError {
                    code: "tethers_engine_error".into(),
                    message: format!("{e}"),
                },
            );
        }
    };

    // 5. Return the complete typed response — no Action execution.
    #[allow(clippy::expect_used)]
    let body =
        serde_json::to_value(&response).expect("TethersResponse serialization must not fail");
    (StatusCode::OK, Json(body))
}

fn error_response(status: StatusCode, error: ApiError) -> (StatusCode, Json<serde_json::Value>) {
    #[allow(clippy::expect_used)]
    let body = serde_json::to_value(&error).expect("ApiError serialization must not fail");
    (status, Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the response DTOs serialize correctly through Axum JSON.
    #[test]
    fn response_dtos_serialize_for_json_output() {
        // Matched
        let matched = TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("e1".into()),
            event_id: Some("ev1".into()),
            tether_id: Some("t1".into()),
            tether_version: Some("v1".into()),
            status: crate::tethers_preview::TethersStatus::Matched,
            plan: Some(crate::tethers_preview::Plan {
                id: "e1/plan".into(),
                required_effects: vec!["lantern.write".into()],
                actions: vec![crate::tethers_preview::PlannedAction {
                    action_id: "action_1".into(),
                    idempotency_key: "e1/action_1".into(),
                    capability: "lantern.task.record".into(),
                    capability_version: "1.0.0".into(),
                    arguments: serde_json::json!({}),
                    effects: vec!["lantern.write".into()],
                }],
            }),
            trail: Some(vec![crate::tethers_preview::TrailEntry {
                sequence: 1,
                phase: "reception".into(),
                kind: "event_received".into(),
                outcome: "accepted".into(),
                message: "received".into(),
            }]),
            error: None,
        };
        let json = serde_json::to_value(&matched).expect("serialize matched");
        assert_eq!(json["status"], "matched");
        assert!(json["plan"].is_object());

        // Not matched
        let not_matched = TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("e2".into()),
            event_id: Some("ev2".into()),
            tether_id: Some("t2".into()),
            tether_version: Some("v2".into()),
            status: crate::tethers_preview::TethersStatus::NotMatched,
            plan: None,
            trail: Some(vec![crate::tethers_preview::TrailEntry {
                sequence: 1,
                phase: "evaluation".into(),
                kind: "condition_checked".into(),
                outcome: "not_matched".into(),
                message: "condition false".into(),
            }]),
            error: None,
        };
        let json = serde_json::to_value(&not_matched).expect("serialize not_matched");
        assert_eq!(json["status"], "not_matched");
        assert!(json["plan"].is_null());

        // Minimal error
        let error_resp = TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: None,
            event_id: None,
            tether_id: None,
            tether_version: None,
            status: crate::tethers_preview::TethersStatus::Error,
            plan: None,
            trail: None,
            error: Some(crate::tethers_preview::TethersError {
                code: "parse_error".into(),
                message: "syntax error".into(),
            }),
        };
        let json = serde_json::to_value(&error_resp).expect("serialize error");
        assert_eq!(json["status"], "error");
        assert_eq!(json["error"]["code"], "parse_error");
        assert!(json.get("evaluation_id").is_none() || json["evaluation_id"].is_null());

        // Correlated error
        let corr = TethersResponse {
            protocol_version: "0.1".into(),
            evaluation_id: Some("e3".into()),
            event_id: Some("ev3".into()),
            tether_id: Some("t3".into()),
            tether_version: Some("v3".into()),
            status: crate::tethers_preview::TethersStatus::Error,
            plan: None,
            trail: Some(vec![crate::tethers_preview::TrailEntry {
                sequence: 1,
                phase: "evaluation".into(),
                kind: "condition_failed".into(),
                outcome: "error".into(),
                message: "missing fact".into(),
            }]),
            error: Some(crate::tethers_preview::TethersError {
                code: "missing_fact".into(),
                message: "fact not found".into(),
            }),
        };
        let json = serde_json::to_value(&corr).expect("serialize correlated");
        assert_eq!(json["status"], "error");
        assert_eq!(json["evaluation_id"], "e3");
        assert!(json["trail"].is_array());
    }

    /// Verify ApiError shapes match the documented contract.
    #[test]
    fn error_contracts_have_expected_shape() {
        let tethers_unavailable = ApiError {
            code: "tethers_unavailable".into(),
            message: "Tethers engine is not configured".into(),
        };
        let json = serde_json::to_value(&tethers_unavailable).expect("serialize");
        assert_eq!(json["code"], "tethers_unavailable");
        assert!(json["message"].is_string());

        let tethers_engine_error = ApiError {
            code: "tethers_engine_error".into(),
            message: "timeout after 10s".into(),
        };
        let json = serde_json::to_value(&tethers_engine_error).expect("serialize");
        assert_eq!(json["code"], "tethers_engine_error");
        // No path leakage in message
        assert!(!json["message"].as_str().unwrap().contains("\\\\"));
        assert!(!json["message"].as_str().unwrap().contains("/bin/"));
    }
}
