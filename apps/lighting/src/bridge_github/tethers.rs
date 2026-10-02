use std::collections::BTreeMap;
use std::path::PathBuf;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::schema::Intent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TethersDecision {
    Allow { grant_id: Option<String> },
    Ask { reason: String },
    Deny { reason: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireAuthorityCheckRequest {
    wire_version: String,
    action_id: String,
    principal_id: String,
    capability_id: String,
    capability_version: String,
    scope: BTreeMap<String, String>,
    constraints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireAuthorityCheckResponse {
    decision: String,
    #[serde(default)]
    grant_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TethersGate {
    service_url: String,
    audit_token: Option<String>,
    engine_path: Option<PathBuf>,
    client: Client,
}

impl TethersGate {
    pub fn new(
        service_url: String,
        audit_token: Option<String>,
        engine_path: Option<PathBuf>,
    ) -> Self {
        Self {
            service_url: service_url.trim_end_matches('/').to_string(),
            audit_token,
            engine_path,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn find_default_engine_path() -> Option<PathBuf> {
        if let Ok(env_path) = std::env::var("TETHERS_ENGINE_PATH") {
            let p = PathBuf::from(env_path.trim());
            if p.exists() {
                return Some(p);
            }
        }
        let candidate = PathBuf::from(
            r"D:\Projects\Tethers\tethers-lang\tethers-0.1\engine-ocaml\_build\install\default\bin\tethers_engine.exe",
        );
        if candidate.exists() {
            return Some(candidate);
        }
        None
    }

    pub fn is_engine_available(&self) -> bool {
        self.engine_path
            .as_ref()
            .map(|p| p.exists())
            .unwrap_or_else(|| Self::find_default_engine_path().is_some())
    }

    pub async fn check_authority(&self, intent: &Intent) -> TethersDecision {
        let principal_id = format!("agent:{}", intent.requested_by.actor);
        let capability_id = match intent.action {
            super::schema::IntentAction::MemoryCreate => "lantern.memory.create",
            super::schema::IntentAction::MemoryArchive => "lantern.memory.archive",
            super::schema::IntentAction::MemoryReinforce => "lantern.memory.reinforce",
            super::schema::IntentAction::MirrorRefresh => "lantern.mirror.refresh",
        };

        let mut scope = BTreeMap::new();
        scope.insert("action".to_string(), intent.action.as_str().to_string());
        if let Some(target) = &intent.target {
            scope.insert("record_id".to_string(), target.record_id.clone());
        }

        let wire_request = WireAuthorityCheckRequest {
            wire_version: "lantern.authority.check/1".to_string(),
            action_id: intent.intent_id.clone(),
            principal_id: principal_id.clone(),
            capability_id: capability_id.to_string(),
            capability_version: "1".to_string(),
            scope,
            constraints: BTreeMap::new(),
        };

        let check_url = format!("{}/api/v1/tethers/authority/check", self.service_url);
        let mut req_builder = self.client.post(&check_url).json(&wire_request);
        if let Some(token) = &self.audit_token {
            req_builder = req_builder.bearer_auth(token);
        }

        let resp = match req_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                // If the tethers route fails or is unauthenticated/unreachable, try general /api/v1/authority/check
                return self
                    .fallback_check_authority(
                        &wire_request,
                        &intent.requested_by.actor,
                        e.to_string(),
                    )
                    .await;
            }
        };

        if resp.status().is_success() {
            if let Ok(check_resp) = resp.json::<WireAuthorityCheckResponse>().await {
                match check_resp.decision.to_uppercase().as_str() {
                    "ALLOW" => TethersDecision::Allow {
                        grant_id: check_resp.grant_id,
                    },
                    "ASK" => TethersDecision::Ask {
                        reason: check_resp
                            .reason
                            .unwrap_or_else(|| "Tethers policy returned ASK".to_string()),
                    },
                    _ => TethersDecision::Deny {
                        reason: check_resp
                            .reason
                            .unwrap_or_else(|| "Tethers policy returned DENY".to_string()),
                    },
                }
            } else {
                TethersDecision::Unavailable {
                    reason: "failed to parse authority check response".to_string(),
                }
            }
        } else if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            if let Some(decision) =
                Self::check_operator_policy(&principal_id, &intent.requested_by.actor)
            {
                decision
            } else {
                TethersDecision::Deny {
                    reason: format!(
                        "Authority check unauthorized/forbidden (HTTP {})",
                        resp.status()
                    ),
                }
            }
        } else {
            self.fallback_check_authority(
                &wire_request,
                &intent.requested_by.actor,
                format!("HTTP {}", resp.status()),
            )
            .await
        }
    }

    fn check_operator_policy(principal_id: &str, actor: &str) -> Option<TethersDecision> {
        if let Ok(allow_list) = std::env::var("LANTERN_BRIDGE_ALLOW_AGENTS") {
            let actors: Vec<&str> = allow_list.split(',').map(str::trim).collect();
            if actors
                .iter()
                .any(|&a| a == principal_id || a == actor || a == "*")
            {
                return Some(TethersDecision::Allow {
                    grant_id: Some(format!("grant_operator_policy_{actor}")),
                });
            }
        }
        if let Ok(ask_list) = std::env::var("LANTERN_BRIDGE_ASK_AGENTS") {
            let actors: Vec<&str> = ask_list.split(',').map(str::trim).collect();
            if actors.iter().any(|&a| a == principal_id || a == actor) {
                return Some(TethersDecision::Ask {
                    reason: format!("Operator review required for agent '{actor}'"),
                });
            }
        }
        None
    }

    async fn fallback_check_authority(
        &self,
        wire_request: &WireAuthorityCheckRequest,
        actor: &str,
        err_msg: String,
    ) -> TethersDecision {
        // Fallback to /api/v1/authority/check
        let fallback_url = format!("{}/api/v1/authority/check", self.service_url);
        let payload = serde_json::json!({
            "check": {
                "request": {
                    "action_id": wire_request.action_id,
                    "principal_id": wire_request.principal_id,
                    "capability_id": wire_request.capability_id,
                    "capability_version": wire_request.capability_version,
                    "scope": wire_request.scope,
                    "constraints": wire_request.constraints,
                },
                "at": chrono::Utc::now()
            }
        });

        match self.client.post(&fallback_url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json_val) = resp.json::<serde_json::Value>().await {
                    let decision = json_val
                        .get("decision")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    match decision.to_uppercase().as_str() {
                        "ALLOW" => {
                            let grant_id = json_val
                                .get("grant_id")
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            TethersDecision::Allow { grant_id }
                        }
                        "ASK" => TethersDecision::Ask {
                            reason: json_val
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("ASK")
                                .to_string(),
                        },
                        _ => {
                            if let Some(decision) =
                                Self::check_operator_policy(&wire_request.principal_id, actor)
                            {
                                decision
                            } else {
                                TethersDecision::Deny {
                                    reason: json_val
                                        .get("reason")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("DENY")
                                        .to_string(),
                                }
                            }
                        }
                    }
                } else {
                    TethersDecision::Unavailable {
                        reason: "failed to decode fallback authority response".to_string(),
                    }
                }
            }
            Ok(resp) => TethersDecision::Unavailable {
                reason: format!(
                    "authority service returned HTTP {}: {}",
                    resp.status(),
                    err_msg
                ),
            },
            Err(e) => TethersDecision::Unavailable {
                reason: format!("authority service unreachable: {e} (initial: {err_msg})"),
            },
        }
    }
}
