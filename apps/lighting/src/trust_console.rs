use std::{
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    Extension, Router,
    extract::Json,
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use chrono::{Duration, Utc};
use lighting_core::{AuthorityCheck, AuthorityRequest, AuthorityScope, PrincipalId};
use lighting_service::{
    AuthorityService,
    authority_dto::{GrantIntent, RevocationIntent},
    authority_ops::AuthorityOperationError,
};
use serde::Serialize;
use serde_json::{Value, json};

const PERSONA_ID: &str = "demo-agent";
const PERSONA_NAME: &str = "Alex Morgan";
const CAPABILITY_ID: &str = "demo.export_summary";
const CAPABILITY_VERSION: &str = "1";
const DEFAULT_SCOPE_VALUE: &str = "sandbox/Lantern Warden M6 demo";
const REPLAY_ARTIFACT: &str =
    include_str!("../../../docs/hackathon/evidence/m5-full-trust-chain.json");
const PAGE: &str = include_str!("../static/trust-console.html");
const CONTROLLED_HOSTILE_PAGE: &str = include_str!("../static/controlled-hostile-page.html");

#[derive(Clone)]
pub struct TrustConsole {
    authority: AuthorityService,
    session: lighting_service::authority_ops::ControlSession,
    state: Arc<tokio::sync::RwLock<ConsoleState>>,
    reset_counter: Arc<AtomicU64>,
}

#[derive(Clone, Debug, Serialize)]
struct ConsoleState {
    reset_id: String,
    phase: &'static str,
    grant: Option<PublicGrant>,
    revocation_id: Option<String>,
    last_decision: Option<DecisionView>,
}

#[derive(Clone, Debug, Serialize)]
struct PublicGrant {
    grant_id: String,
    principal_id: String,
    capability: String,
    scope: Value,
    expires_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
struct DecisionView {
    decision: &'static str,
    reason_code: Option<&'static str>,
    grant_id: Option<String>,
    openshell_invoked: bool,
}

impl TrustConsole {
    pub async fn new(authority: AuthorityService) -> Result<Self, AuthorityOperationError> {
        let session = authority
            .open_control_session(PrincipalId::new("human:demo-judge").expect("fixed principal"))
            .await;
        let console = Self {
            authority,
            session,
            state: Arc::new(tokio::sync::RwLock::new(ConsoleState {
                reset_id: String::new(),
                phase: "READY",
                grant: None,
                revocation_id: None,
                last_decision: None,
            })),
            reset_counter: Arc::new(AtomicU64::new(0)),
        };
        console.reset().await?;
        Ok(console)
    }

    async fn reset(&self) -> Result<ConsoleState, AuthorityOperationError> {
        // Reset is deliberately scoped to the synthetic demo principal. Each
        // revocation uses the same authenticated control-plane method as the
        // normal authority route; the browser never receives session secrets.
        for grant in self.authority.list_grants().await? {
            if grant.delegate_principal_id.as_str() == PERSONA_ID
                && grant.capability_id == CAPABILITY_ID
                && self
                    .authority
                    .explain(&grant.id)
                    .await?
                    .is_some_and(|(_, revocation)| revocation.is_none())
            {
                let _ = self
                    .authority
                    .revoke_from_control_plane(
                        &self.session.session_id,
                        &self.session.csrf_token,
                        RevocationIntent { grant_id: grant.id },
                    )
                    .await?;
            }
        }
        let reset_id = format!(
            "m6-demo-reset-{}",
            self.reset_counter.fetch_add(1, Ordering::Relaxed) + 1
        );
        let state = ConsoleState {
            reset_id,
            phase: "READY",
            grant: None,
            revocation_id: None,
            last_decision: None,
        };
        *self.state.write().await = state.clone();
        Ok(state)
    }

    async fn attack(&self) -> ConsoleState {
        let mut state = self.state.write().await;
        state.phase = "ATTACKED";
        state.last_decision = Some(DecisionView {
            decision: "DENY",
            reason_code: Some("NO_AUTHENTICATED_GRANT"),
            grant_id: None,
            openshell_invoked: false,
        });
        state.clone()
    }

    async fn grant(&self) -> Result<ConsoleState, AuthorityOperationError> {
        let grant = self
            .authority
            .issue_from_control_plane(
                &self.session.session_id,
                &self.session.csrf_token,
                GrantIntent {
                    delegate_principal_id: PrincipalId::new(PERSONA_ID)
                        .expect("fixed demo principal"),
                    capability_id: CAPABILITY_ID.to_owned(),
                    capability_version: CAPABILITY_VERSION.to_owned(),
                    scope: demo_scope(DEFAULT_SCOPE_VALUE),
                    expires_at: Some(Utc::now() + Duration::minutes(10)),
                    constraints: Default::default(),
                },
            )
            .await?;
        let mut state = self.state.write().await;
        state.phase = "GRANTED";
        state.grant = Some(public_grant(&grant));
        state.revocation_id = None;
        state.last_decision = None;
        Ok(state.clone())
    }

    async fn wrong_scope(&self) -> Result<ConsoleState, AuthorityOperationError> {
        let decision = self
            .authority
            .check(&AuthorityCheck {
                request: AuthorityRequest {
                    action_id: "m6-wrong-scope".to_owned(),
                    principal_id: PrincipalId::new(PERSONA_ID).expect("fixed demo principal"),
                    capability_id: CAPABILITY_ID.to_owned(),
                    capability_version: CAPABILITY_VERSION.to_owned(),
                    scope: demo_scope("sandbox/not-the-approved-destination"),
                    constraints: Default::default(),
                },
                at: Utc::now(),
            })
            .await?;
        let mut state = self.state.write().await;
        state.phase = "WRONG_SCOPE";
        state.last_decision = Some(decision_view(&decision));
        Ok(state.clone())
    }

    async fn retry(&self) -> Result<Value, AuthorityOperationError> {
        let decision = self
            .authority
            .check(&AuthorityCheck {
                request: AuthorityRequest {
                    action_id: "m6-retry".to_owned(),
                    principal_id: PrincipalId::new(PERSONA_ID).expect("fixed demo principal"),
                    capability_id: CAPABILITY_ID.to_owned(),
                    capability_version: CAPABILITY_VERSION.to_owned(),
                    scope: demo_scope(DEFAULT_SCOPE_VALUE),
                    constraints: Default::default(),
                },
                at: Utc::now(),
            })
            .await?;
        let mut state = self.state.write().await;
        let view = decision_view(&decision);
        state.last_decision = Some(view.clone());
        match decision {
            lighting_core::AuthorityDecision::Allow { ref grant_id, .. } => {
                state.phase = "SUCCEEDED_REPLAY";
                Ok(json!({
                    "status": "success",
                    "mode": "REPLAY",
                    "authority": view,
                    "openshell": "EXECUTED",
                    "trail": "SUCCESS",
                    "receipt": "VERIFIED",
                    "grant_id": grant_id,
                    "replay_reference": "docs/hackathon/evidence/m5-full-trust-chain.json",
                    "execution_reference": "exec_00000000-0000-4000-8000-000000000001",
                    "artifact": "/sandbox/outbox/approved/summary.txt"
                }))
            }
            lighting_core::AuthorityDecision::Deny { .. } => Ok(json!({
                "status": "denied",
                "mode": "REPLAY",
                "authority": view,
                "openshell": "NOT INVOKED",
                "effect": "NONE"
            })),
        }
    }

    async fn revoke(&self) -> Result<ConsoleState, AuthorityOperationError> {
        let grant_id = self
            .state
            .read()
            .await
            .grant
            .as_ref()
            .map(|grant| grant.grant_id.clone())
            .ok_or_else(|| AuthorityOperationError::Invalid("no demo grant exists".to_owned()))?;
        let revocation = self
            .authority
            .revoke_from_control_plane(
                &self.session.session_id,
                &self.session.csrf_token,
                RevocationIntent {
                    grant_id: lighting_core::AuthorityGrantId::new(grant_id)
                        .map_err(|error| AuthorityOperationError::Invalid(error.to_string()))?,
                },
            )
            .await?;
        let mut state = self.state.write().await;
        state.phase = "REVOKED";
        state.revocation_id = Some(revocation.id.to_string());
        state.last_decision = Some(DecisionView {
            decision: "DENY",
            reason_code: Some("GRANT_REVOKED"),
            grant_id: state.grant.as_ref().map(|grant| grant.grant_id.clone()),
            openshell_invoked: false,
        });
        Ok(state.clone())
    }

    async fn public_state(&self) -> Result<Value, AuthorityOperationError> {
        let state = self.state.read().await.clone();
        replay_payload(
            state,
            if is_public_demo() {
                "public-demo"
            } else {
                "local-demo"
            },
            !is_public_demo()
                && std::env::var("TAVILY_API_KEY").is_ok()
                && std::env::var("NEBIUS_API_KEY").is_ok(),
        )
    }
}

/// Replay-only public console adapter. It has no AuthorityService, control
/// session, or mutation path; its actions only move the synthetic display
/// through the accepted M4/M5 evidence sequence.
#[derive(Clone)]
pub struct PublicReplayConsole {
    state: Arc<tokio::sync::RwLock<ConsoleState>>,
    reset_counter: Arc<AtomicU64>,
}

impl PublicReplayConsole {
    pub fn new() -> Self {
        Self {
            state: Arc::new(tokio::sync::RwLock::new(ConsoleState {
                reset_id: "m7-public-reset-0".to_owned(),
                phase: "READY",
                grant: None,
                revocation_id: None,
                last_decision: None,
            })),
            reset_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn state(&self) -> Result<Value, AuthorityOperationError> {
        replay_payload(self.state.read().await.clone(), "public-demo", false)
    }

    async fn reset(&self) -> ConsoleState {
        let state = ConsoleState {
            reset_id: format!(
                "m7-public-reset-{}",
                self.reset_counter.fetch_add(1, Ordering::Relaxed) + 1
            ),
            phase: "READY",
            grant: None,
            revocation_id: None,
            last_decision: None,
        };
        *self.state.write().await = state.clone();
        state
    }

    async fn attack(&self) -> ConsoleState {
        let mut state = self.state.write().await;
        state.phase = "ATTACKED";
        state.last_decision = Some(DecisionView {
            decision: "DENY",
            reason_code: Some("NO_AUTHENTICATED_GRANT"),
            grant_id: None,
            openshell_invoked: false,
        });
        state.clone()
    }

    async fn grant(&self) -> ConsoleState {
        let mut state = self.state.write().await;
        state.phase = "GRANTED";
        state.grant = Some(accepted_replay_grant());
        state.revocation_id = None;
        state.last_decision = None;
        state.clone()
    }

    async fn wrong_scope(&self) -> ConsoleState {
        let mut state = self.state.write().await;
        state.phase = "WRONG_SCOPE";
        state.last_decision = Some(DecisionView {
            decision: "DENY",
            reason_code: Some("SCOPE_MISMATCH"),
            grant_id: None,
            openshell_invoked: false,
        });
        state.clone()
    }

    async fn retry(&self) -> Value {
        let mut state = self.state.write().await;
        let grant_id = state.grant.as_ref().map(|grant| grant.grant_id.clone());
        let revoked = state.phase == "REVOKED";
        if let Some(grant_id) = grant_id.filter(|_| !revoked) {
            state.phase = "SUCCEEDED_REPLAY";
            state.last_decision = Some(DecisionView {
                decision: "ALLOW",
                reason_code: None,
                grant_id: Some(grant_id.clone()),
                openshell_invoked: false,
            });
            json!({
                "status": "success",
                "mode": "REPLAY",
                "authority": state.last_decision,
                "openshell": "EXECUTED",
                "trail": "SUCCESS",
                "receipt": "VERIFIED",
                "grant_id": grant_id,
                "replay_reference": "docs/hackathon/evidence/m5-full-trust-chain.json",
                "execution_reference": "exec_00000000-0000-4000-8000-000000000001",
                "artifact": "/sandbox/outbox/approved/summary.txt"
            })
        } else {
            state.last_decision = Some(DecisionView {
                decision: "DENY",
                reason_code: Some(if revoked {
                    "GRANT_REVOKED"
                } else {
                    "NO_AUTHENTICATED_GRANT"
                }),
                grant_id: None,
                openshell_invoked: false,
            });
            json!({
                "status": "denied",
                "mode": "REPLAY",
                "authority": state.last_decision,
                "openshell": "NOT INVOKED",
                "effect": "NONE"
            })
        }
    }

    async fn revoke(&self) -> ConsoleState {
        let mut state = self.state.write().await;
        state.phase = "REVOKED";
        state.revocation_id = Some("accepted-m5-revocation".to_owned());
        state.last_decision = Some(DecisionView {
            decision: "DENY",
            reason_code: Some("GRANT_REVOKED"),
            grant_id: state.grant.as_ref().map(|grant| grant.grant_id.clone()),
            openshell_invoked: false,
        });
        state.clone()
    }
}

impl Default for PublicReplayConsole {
    fn default() -> Self {
        Self::new()
    }
}

pub fn router(console: TrustConsole) -> Router {
    Router::new()
        .route("/console", get(page))
        .route(
            "/console/controlled-hostile-page",
            get(controlled_hostile_page),
        )
        .route("/api/v1/console/state", get(state))
        .route("/api/v1/console/reset", post(reset))
        .route("/api/v1/console/attack", post(attack))
        .route("/api/v1/console/grant", post(grant))
        .route("/api/v1/console/wrong-scope", post(wrong_scope))
        .route("/api/v1/console/retry", post(retry))
        .route("/api/v1/console/revoke", post(revoke))
        .layer(Extension(Arc::new(console)))
}

pub fn public_router(console: PublicReplayConsole) -> Router {
    Router::new()
        .route("/console", get(page))
        .route(
            "/console/controlled-hostile-page",
            get(controlled_hostile_page),
        )
        .route("/api/v1/console/state", get(public_state))
        .route("/api/v1/console/replay/state", get(public_state))
        .route("/api/v1/console/replay/reset", post(public_reset))
        .route("/api/v1/console/replay/attack", post(public_attack))
        .route("/api/v1/console/replay/grant", post(public_replay_grant))
        .route(
            "/api/v1/console/replay/wrong-scope",
            post(public_wrong_scope),
        )
        .route("/api/v1/console/replay/retry", post(public_retry))
        .route("/api/v1/console/replay/revoke", post(public_revoke))
        .layer(Extension(Arc::new(console)))
}

async fn page() -> Html<&'static str> {
    Html(PAGE)
}

async fn controlled_hostile_page() -> Html<&'static str> {
    Html(CONTROLLED_HOSTILE_PAGE)
}

async fn state(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    console
        .public_state()
        .await
        .map(Json)
        .map_err(error_response)
}

async fn public_state(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    console.state().await.map(Json).map_err(error_response)
}

async fn public_reset(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Json<ConsoleState> {
    Json(console.reset().await)
}

async fn public_attack(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Json<ConsoleState> {
    Json(console.attack().await)
}

async fn public_replay_grant(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Json<ConsoleState> {
    Json(console.grant().await)
}

async fn public_wrong_scope(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Json<ConsoleState> {
    Json(console.wrong_scope().await)
}

async fn public_retry(Extension(console): Extension<Arc<PublicReplayConsole>>) -> Json<Value> {
    Json(console.retry().await)
}

async fn public_revoke(
    Extension(console): Extension<Arc<PublicReplayConsole>>,
) -> Json<ConsoleState> {
    Json(console.revoke().await)
}

async fn reset(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<ConsoleState>, (StatusCode, Json<Value>)> {
    console.reset().await.map(Json).map_err(error_response)
}

async fn attack(Extension(console): Extension<Arc<TrustConsole>>) -> Json<ConsoleState> {
    Json(console.attack().await)
}

async fn grant(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<ConsoleState>, (StatusCode, Json<Value>)> {
    console.grant().await.map(Json).map_err(error_response)
}

async fn wrong_scope(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<ConsoleState>, (StatusCode, Json<Value>)> {
    console
        .wrong_scope()
        .await
        .map(Json)
        .map_err(error_response)
}

async fn retry(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    console.retry().await.map(Json).map_err(error_response)
}

async fn revoke(
    Extension(console): Extension<Arc<TrustConsole>>,
) -> Result<Json<ConsoleState>, (StatusCode, Json<Value>)> {
    console.revoke().await.map(Json).map_err(error_response)
}

fn demo_scope(value: &str) -> AuthorityScope {
    AuthorityScope::from_pairs([
        ("kind", "path_prefix"),
        ("value", value),
        ("argument_json_pointer", "/summary"),
        ("allowed_prefixes", "sandbox/"),
    ])
    .expect("fixed demo scope")
}

fn is_public_demo() -> bool {
    std::env::var("WARDEN_PUBLIC_DEMO").ok().as_deref() == Some("1")
}

fn replay_payload(
    state: ConsoleState,
    deployment_mode: &str,
    live_cognition: bool,
) -> Result<Value, AuthorityOperationError> {
    Ok(json!({
        "mode": "REPLAY",
        "deployment_mode": deployment_mode,
        "live_cognition": live_cognition,
        "persona": {
            "name": PERSONA_NAME,
            "id": PERSONA_ID,
            "preference": "Keep weekly summaries local."
        },
        "state": state,
        "providers": {
            "tavily": if live_cognition { "LIVE" } else { "OFFLINE" },
            "nebius_nemotron": if live_cognition { "LIVE" } else { "OFFLINE" },
            "lantern_warden": "READY",
            "tethers": "REPLAY",
            "openshell": "REPLAY"
        },
        "replay_artifact": serde_json::from_str::<Value>(REPLAY_ARTIFACT)
            .map_err(|error| AuthorityOperationError::Invalid(error.to_string()))?
    }))
}

fn accepted_replay_grant() -> PublicGrant {
    let artifact = serde_json::from_str::<Value>(REPLAY_ARTIFACT)
        .expect("committed replay artifact must be valid JSON");
    let accepted = &artifact["authorised_attempt"];
    PublicGrant {
        grant_id: accepted["grant_id"]
            .as_str()
            .unwrap_or("accepted-m5-grant")
            .to_owned(),
        principal_id: accepted["principal_id"]
            .as_str()
            .unwrap_or("accepted-replay")
            .to_owned(),
        capability: accepted["capability"]
            .as_str()
            .unwrap_or("demo.export_summary@1")
            .to_owned(),
        scope: accepted["scope"].clone(),
        expires_at: None,
    }
}

fn public_grant(grant: &lighting_core::AuthorityGrant) -> PublicGrant {
    PublicGrant {
        grant_id: grant.id.to_string(),
        principal_id: grant.delegate_principal_id.to_string(),
        capability: format!("{}@{}", grant.capability_id, grant.capability_version),
        scope: serde_json::to_value(&grant.scope).expect("scope serializes"),
        expires_at: grant.expires_at,
    }
}

fn decision_view(decision: &lighting_core::AuthorityDecision) -> DecisionView {
    match decision {
        lighting_core::AuthorityDecision::Allow { grant_id, .. } => DecisionView {
            decision: "ALLOW",
            reason_code: None,
            grant_id: Some(grant_id.to_string()),
            openshell_invoked: false,
        },
        lighting_core::AuthorityDecision::Deny { reason } => DecisionView {
            decision: "DENY",
            reason_code: Some(reason_code(reason)),
            grant_id: None,
            openshell_invoked: false,
        },
    }
}

fn reason_code(reason: &lighting_core::DenyReason) -> &'static str {
    match reason {
        lighting_core::DenyReason::NoMatchingGrant => "NO_AUTHENTICATED_GRANT",
        lighting_core::DenyReason::ExpiredGrant => "EXPIRED_GRANT",
        lighting_core::DenyReason::RevokedGrant => "GRANT_REVOKED",
        lighting_core::DenyReason::PrincipalMismatch => "PRINCIPAL_MISMATCH",
        lighting_core::DenyReason::CapabilityMismatch => "CAPABILITY_MISMATCH",
        lighting_core::DenyReason::ScopeMismatch => "SCOPE_MISMATCH",
        lighting_core::DenyReason::ConstraintMismatch => "CONSTRAINT_MISMATCH",
    }
}

fn error_response(error: AuthorityOperationError) -> (StatusCode, Json<Value>) {
    (
        match error {
            AuthorityOperationError::Unavailable | AuthorityOperationError::Persistence(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            AuthorityOperationError::Unauthenticated
            | AuthorityOperationError::InvalidCsrf
            | AuthorityOperationError::ExpiredSession => StatusCode::UNAUTHORIZED,
            AuthorityOperationError::Invalid(_) => StatusCode::BAD_REQUEST,
        },
        Json(json!({"code": "trust_console_error", "message": error.to_string()})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_page_and_replay_artifact_do_not_contain_credentials() {
        for secret_name in [
            "NEBIUS_API_KEY",
            "TAVILY_API_KEY",
            "LANTERN_TETHERS_AUDIT_TOKEN",
        ] {
            assert!(!PAGE.contains(secret_name));
        }
        assert!(!CONTROLLED_HOSTILE_PAGE.contains("Matthew"));
        assert!(!CONTROLLED_HOSTILE_PAGE.contains("NEBIUS_API_KEY"));
        assert!(!CONTROLLED_HOSTILE_PAGE.contains("TAVILY_API_KEY"));
        assert!(PAGE.contains("CANDIDATE ONLY"));
        assert!(PAGE.contains("REPLAY"));
        for unavailable_message in [
            "EXTERNAL SEARCH UNAVAILABLE",
            "SEMANTIC INTERPRETATION UNAVAILABLE",
            "AUTHORITY UNAVAILABLE",
            "EXECUTION UNAVAILABLE",
        ] {
            assert!(PAGE.contains(unavailable_message));
        }
        let artifact = serde_json::from_str::<Value>(REPLAY_ARTIFACT).unwrap();
        assert_eq!(artifact["secrets_included"], false);
    }

    #[test]
    fn wrong_scope_is_named_without_collapsing_into_generic_confidence() {
        assert_eq!(
            reason_code(&lighting_core::DenyReason::ScopeMismatch),
            "SCOPE_MISMATCH"
        );
    }

    #[tokio::test]
    async fn reset_revokes_only_synthetic_demo_grants() {
        let authority = AuthorityService::new();
        let console = TrustConsole::new(authority).await.unwrap();
        console.grant().await.unwrap();
        let reset = console.reset().await.unwrap();
        assert_eq!(reset.phase, "READY");
        assert!(reset.grant.is_none());
        assert_eq!(console.authority.list_grants().await.unwrap().len(), 1);
        assert!(
            console
                .authority
                .list_revocations()
                .await
                .unwrap()
                .iter()
                .any(|revocation| revocation.grant_id.to_string().starts_with("grant_"))
        );
    }

    #[tokio::test]
    async fn retry_without_grant_is_denied_without_effect() {
        let console = TrustConsole::new(AuthorityService::new()).await.unwrap();
        let result = console.retry().await.unwrap();
        assert_eq!(result["status"], "denied");
        assert_eq!(result["authority"]["decision"], "DENY");
        assert_eq!(result["authority"]["reason_code"], "NO_AUTHENTICATED_GRANT");
        assert_eq!(result["openshell"], "NOT INVOKED");
        assert_eq!(result["effect"], "NONE");
    }

    #[tokio::test]
    async fn grant_and_revoke_use_real_control_plane_mutations() {
        let console = TrustConsole::new(AuthorityService::new()).await.unwrap();
        let granted = console.grant().await.unwrap();
        assert_eq!(granted.phase, "GRANTED");
        let success = console.retry().await.unwrap();
        assert_eq!(success["status"], "success");
        assert_eq!(success["authority"]["decision"], "ALLOW");
        assert_eq!(success["openshell"], "EXECUTED");

        let wrong_scope = console.wrong_scope().await.unwrap();
        assert_eq!(
            wrong_scope.last_decision.unwrap().reason_code,
            Some("SCOPE_MISMATCH")
        );

        let revoked = console.revoke().await.unwrap();
        assert_eq!(revoked.phase, "REVOKED");
        let after_revoke = console.retry().await.unwrap();
        assert_eq!(after_revoke["status"], "denied");
        assert_eq!(after_revoke["authority"]["reason_code"], "GRANT_REVOKED");
        assert_eq!(after_revoke["openshell"], "NOT INVOKED");
    }

    #[tokio::test]
    async fn public_payload_is_sanitized_and_provider_failures_do_not_create_evidence() {
        let console = TrustConsole::new(AuthorityService::new()).await.unwrap();
        let payload = console.public_state().await.unwrap();
        let encoded = serde_json::to_string(&payload).unwrap();
        for secret_name in [
            "NEBIUS_API_KEY",
            "TAVILY_API_KEY",
            "LANTERN_TETHERS_AUDIT_TOKEN",
        ] {
            assert!(!encoded.contains(secret_name));
        }
        assert_eq!(payload["mode"], "REPLAY");
        assert_eq!(payload["replay_artifact"]["secrets_included"], false);
        assert_eq!(
            payload["replay_artifact"]["cognitive_evidence"]["authority_changed"],
            false
        );
    }

    #[tokio::test]
    async fn public_replay_flow_is_labelled_and_has_no_authority_service() {
        let console = PublicReplayConsole::new();
        let initial = console.state().await.unwrap();
        assert_eq!(initial["deployment_mode"], "public-demo");
        assert_eq!(initial["live_cognition"], false);

        console.attack().await;
        let denied = console.retry().await;
        assert_eq!(denied["mode"], "REPLAY");
        assert_eq!(denied["authority"]["reason_code"], "NO_AUTHENTICATED_GRANT");

        console.grant().await;
        let allowed = console.retry().await;
        assert_eq!(allowed["mode"], "REPLAY");
        assert_eq!(allowed["status"], "success");
        assert_eq!(allowed["openshell"], "EXECUTED");

        console.revoke().await;
        let after_revoke = console.retry().await;
        assert_eq!(after_revoke["authority"]["reason_code"], "GRANT_REVOKED");
        assert_eq!(after_revoke["openshell"], "NOT INVOKED");
    }
}
