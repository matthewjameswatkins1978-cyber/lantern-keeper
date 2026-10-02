use std::path::PathBuf;

use chrono::Utc;
use reqwest::Client;
use sha2::{Digest, Sha256};

use super::queue::DiscoveredIntentCommit;
use super::schema::{
    Intent, IntentAction, MirrorRefreshStatus, Receipt, ReceiptAuthority, ReceiptLantern,
    ReceiptStatus, SchemaError, parse_and_validate_intent,
};
use super::state::{BridgeStateStore, ProcessingPhase};
use super::tethers::{TethersDecision, TethersGate};

pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct BridgeExecutor {
    service_url: String,
    client: Client,
    tethers: TethersGate,
    mirror_repo_path: Option<PathBuf>,
}

impl BridgeExecutor {
    pub fn new(
        service_url: String,
        tethers: TethersGate,
        mirror_repo_path: Option<PathBuf>,
    ) -> Self {
        Self {
            service_url: service_url.trim_end_matches('/').to_string(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            tethers,
            mirror_repo_path,
        }
    }

    pub fn compute_canonical_digest(intent: &Intent) -> String {
        let mut hasher = Sha256::new();
        hasher.update(intent.action.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(intent.requested_by.actor.as_bytes());
        hasher.update(b"|");
        if let Some(target) = &intent.target {
            hasher.update(target.record_id.as_bytes());
        }
        hasher.update(b"|");
        let payload_canon = serde_json::to_string(&intent.payload).unwrap_or_default();
        hasher.update(payload_canon.as_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    pub async fn process_discovered_intent(
        &self,
        discovered: &DiscoveredIntentCommit,
        state: &mut BridgeStateStore,
    ) -> Receipt {
        let intent = match parse_and_validate_intent(&discovered.intent_raw_json) {
            Ok(intent) => intent,
            Err(e) => {
                let status = match e {
                    SchemaError::ForbiddenPattern => ReceiptStatus::Suspect,
                    _ => ReceiptStatus::Failed,
                };
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: format!("invalid_{}", &discovered.commit_sha[..8]),
                    intent_commit: discovered.commit_sha.clone(),
                    status,
                    authority: ReceiptAuthority {
                        decision: "DENY".to_string(),
                        reason: Some(format!("schema validation failed: {e}")),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: Some(e.to_string()),
                };
                return receipt;
            }
        };

        // File name must match intent ID
        let expected_filename = format!("{}.json", intent.intent_id);
        if !discovered.intent_file_path.ends_with(&expected_filename) {
            let receipt = Receipt {
                schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                intent_id: intent.intent_id.clone(),
                intent_commit: discovered.commit_sha.clone(),
                status: ReceiptStatus::Suspect,
                authority: ReceiptAuthority {
                    decision: "DENY".to_string(),
                    reason: Some("intent file name does not match intent_id".to_string()),
                },
                lantern: None,
                processed_at: Utc::now(),
                bridge_version: BRIDGE_VERSION.to_string(),
                mirror_refresh: MirrorRefreshStatus::NotRequested,
                error: Some("intent file path mismatch".to_string()),
            };
            let _ = state.record_terminal(&intent.intent_id, receipt.clone());
            return receipt;
        }

        let canonical_digest = Self::compute_canonical_digest(&intent);

        // 1. Idempotency Check
        if let Some(existing) = state.get_idempotency(&intent.intent_id) {
            if existing.canonical_digest != canonical_digest {
                // Same intent ID with different request -> SUSPECT
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Suspect,
                    authority: ReceiptAuthority {
                        decision: "DENY".to_string(),
                        reason: Some(
                            "intent_id already exists with different canonical request".to_string(),
                        ),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: Some(
                        "idempotency conflict: payload mismatch for existing intent_id".to_string(),
                    ),
                };
                return receipt;
            }

            // If already completed or applied, return existing receipt without repeating mutation!
            if let Some(ref receipt) = existing.receipt {
                return receipt.clone();
            }

            if existing.phase == ProcessingPhase::Applied {
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Applied,
                    authority: ReceiptAuthority {
                        decision: "ALLOW".to_string(),
                        reason: None,
                    },
                    lantern: Some(ReceiptLantern {
                        record_id: existing.lantern_record_id.clone(),
                        result_sha256: existing.lantern_result_sha256.clone(),
                    }),
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: None,
                };
                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                return receipt;
            }
        }

        // 2. Preconditions & Policy constraints
        match intent.action {
            IntentAction::MemoryReinforce => {
                // Safety invariant from packet: Lantern Keeper currently lacks an atomic,
                // idempotent reinforcement storage seam. Explicitly disabled with evidence.
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Denied,
                    authority: ReceiptAuthority {
                        decision: "DENY".to_string(),
                        reason: Some("memory.reinforce is currently disabled: Lantern Keeper lacks an atomic, idempotent reinforcement storage seam".to_string()),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: Some("action_disabled: reinforcement storage seam deferred".to_string()),
                };
                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                return receipt;
            }
            IntentAction::MemoryArchive => {
                // Precondition check: live record check & hash match
                let target_id = intent.target.as_ref().unwrap().record_id.clone();
                let live_record = self.fetch_live_memory(&target_id).await;
                match live_record {
                    Ok(Some(mem)) => {
                        let status_str = mem.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        if status_str != "active" {
                            let receipt = Receipt {
                                schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                                intent_id: intent.intent_id.clone(),
                                intent_commit: discovered.commit_sha.clone(),
                                status: ReceiptStatus::Conflict,
                                authority: ReceiptAuthority {
                                    decision: "DENY".to_string(),
                                    reason: Some(format!(
                                        "record '{target_id}' is already {status_str}"
                                    )),
                                },
                                lantern: None,
                                processed_at: Utc::now(),
                                bridge_version: BRIDGE_VERSION.to_string(),
                                mirror_refresh: MirrorRefreshStatus::NotRequested,
                                error: Some(format!(
                                    "optimistic concurrency conflict: record is {status_str}"
                                )),
                            };
                            let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                            return receipt;
                        }

                        // Compute live content hash to check against observed hash
                        let observed_hash = intent
                            .observed
                            .as_ref()
                            .and_then(|o| o.record_sha256.as_deref());
                        if let Some(obs_hash) = observed_hash {
                            let clean_obs = obs_hash.strip_prefix("sha256:").unwrap_or(obs_hash);
                            if !self.check_observed_hash(clean_obs, &target_id, &mem) {
                                let receipt = Receipt {
                                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                                    intent_id: intent.intent_id.clone(),
                                    intent_commit: discovered.commit_sha.clone(),
                                    status: ReceiptStatus::Conflict,
                                    authority: ReceiptAuthority {
                                        decision: "DENY".to_string(),
                                        reason: Some("live record digest does not match observed mirror snapshot".to_string()),
                                    },
                                    lantern: None,
                                    processed_at: Utc::now(),
                                    bridge_version: BRIDGE_VERSION.to_string(),
                                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                                    error: Some("optimistic concurrency conflict: record hash mismatch".to_string()),
                                };
                                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                                return receipt;
                            }
                        }
                    }
                    Ok(None) => {
                        let receipt = Receipt {
                            schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                            intent_id: intent.intent_id.clone(),
                            intent_commit: discovered.commit_sha.clone(),
                            status: ReceiptStatus::Conflict,
                            authority: ReceiptAuthority {
                                decision: "DENY".to_string(),
                                reason: Some(format!(
                                    "target record '{target_id}' not found in Lantern"
                                )),
                            },
                            lantern: None,
                            processed_at: Utc::now(),
                            bridge_version: BRIDGE_VERSION.to_string(),
                            mirror_refresh: MirrorRefreshStatus::NotRequested,
                            error: Some("record_not_found".to_string()),
                        };
                        let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                        return receipt;
                    }
                    Err(e) => {
                        let receipt = Receipt {
                            schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                            intent_id: intent.intent_id.clone(),
                            intent_commit: discovered.commit_sha.clone(),
                            status: ReceiptStatus::Failed,
                            authority: ReceiptAuthority {
                                decision: "DENY".to_string(),
                                reason: Some(format!("failed to query Lantern live record: {e}")),
                            },
                            lantern: None,
                            processed_at: Utc::now(),
                            bridge_version: BRIDGE_VERSION.to_string(),
                            mirror_refresh: MirrorRefreshStatus::NotRequested,
                            error: Some(e),
                        };
                        let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                        return receipt;
                    }
                }
            }
            IntentAction::MemoryCreate | IntentAction::MirrorRefresh => {
                // Preconditions valid
            }
        }

        // 3. Tethers Authority Check
        let decision = self.tethers.check_authority(&intent).await;
        match decision {
            TethersDecision::Allow { grant_id: _ } => {
                // Proceed to mutation
            }
            TethersDecision::Ask { reason } => {
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::RequiresApproval,
                    authority: ReceiptAuthority {
                        decision: "ASK".to_string(),
                        reason: Some(reason),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: None,
                };
                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                return receipt;
            }
            TethersDecision::Deny { reason } => {
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Denied,
                    authority: ReceiptAuthority {
                        decision: "DENY".to_string(),
                        reason: Some(reason),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: None,
                };
                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                return receipt;
            }
            TethersDecision::Unavailable { reason } => {
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Denied,
                    authority: ReceiptAuthority {
                        decision: "DENY".to_string(),
                        reason: Some(format!(
                            "Tethers authority unavailable (fail closed): {reason}"
                        )),
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::NotRequested,
                    error: Some("tethers_unavailable_fail_closed".to_string()),
                };
                let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                return receipt;
            }
        }

        // 4. Record Started in durable state
        let _ = state.record_started(&intent, &discovered.commit_sha, &canonical_digest);

        // 5. Canonical Mutation Execution
        match intent.action {
            IntentAction::MemoryCreate => {
                let content = intent
                    .payload
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let kind = intent
                    .payload
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("fact");
                let project_id = intent.payload.get("project_id").and_then(|v| v.as_str());
                let confidence = intent
                    .payload
                    .get("confidence")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.8) as f32;
                let importance = intent
                    .payload
                    .get("importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5) as f32;

                let remember_req = serde_json::json!({
                    "content": content,
                    "kind": kind,
                    "project_id": project_id,
                    "confidence": confidence,
                    "importance": importance,
                    "agent": intent.requested_by.actor,
                });

                let remember_url = format!("{}/api/v1/memories", self.service_url);
                let resp = match self
                    .client
                    .post(&remember_url)
                    .json(&remember_req)
                    .send()
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        let receipt = Receipt {
                            schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                            intent_id: intent.intent_id.clone(),
                            intent_commit: discovered.commit_sha.clone(),
                            status: ReceiptStatus::Failed,
                            authority: ReceiptAuthority {
                                decision: "ALLOW".to_string(),
                                reason: None,
                            },
                            lantern: None,
                            processed_at: Utc::now(),
                            bridge_version: BRIDGE_VERSION.to_string(),
                            mirror_refresh: MirrorRefreshStatus::NotRequested,
                            error: Some(format!("Lantern remember API call failed: {e}")),
                        };
                        let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                        return receipt;
                    }
                };

                if resp.status().is_success() {
                    let mem_resp: serde_json::Value = resp.json().await.unwrap_or_default();
                    let mem = mem_resp.get("memory").unwrap_or(&mem_resp);
                    let record_id = mem
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let result_digest =
                        format!("sha256:{:x}", Sha256::digest(mem.to_string().as_bytes()));

                    let receipt = Receipt {
                        schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                        intent_id: intent.intent_id.clone(),
                        intent_commit: discovered.commit_sha.clone(),
                        status: ReceiptStatus::Applied,
                        authority: ReceiptAuthority {
                            decision: "ALLOW".to_string(),
                            reason: None,
                        },
                        lantern: Some(ReceiptLantern {
                            record_id: Some(record_id.clone()),
                            result_sha256: Some(result_digest.clone()),
                        }),
                        processed_at: Utc::now(),
                        bridge_version: BRIDGE_VERSION.to_string(),
                        mirror_refresh: MirrorRefreshStatus::Requested,
                        error: None,
                    };
                    let _ = state.record_applied(
                        &intent.intent_id,
                        Some(&record_id),
                        Some(&result_digest),
                        receipt.clone(),
                    );
                    receipt
                } else {
                    let status_code = resp.status();
                    let err_text = resp.text().await.unwrap_or_default();
                    let receipt = Receipt {
                        schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                        intent_id: intent.intent_id.clone(),
                        intent_commit: discovered.commit_sha.clone(),
                        status: ReceiptStatus::Failed,
                        authority: ReceiptAuthority {
                            decision: "ALLOW".to_string(),
                            reason: None,
                        },
                        lantern: None,
                        processed_at: Utc::now(),
                        bridge_version: BRIDGE_VERSION.to_string(),
                        mirror_refresh: MirrorRefreshStatus::NotRequested,
                        error: Some(format!("Lantern HTTP {status_code}: {err_text}")),
                    };
                    let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                    receipt
                }
            }
            IntentAction::MemoryArchive => {
                let target_id = intent.target.as_ref().unwrap().record_id.clone();
                let supersede_url =
                    format!("{}/api/v1/memories/{target_id}/supersede", self.service_url);
                let resp = match self.client.post(&supersede_url).send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        let receipt = Receipt {
                            schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                            intent_id: intent.intent_id.clone(),
                            intent_commit: discovered.commit_sha.clone(),
                            status: ReceiptStatus::Failed,
                            authority: ReceiptAuthority {
                                decision: "ALLOW".to_string(),
                                reason: None,
                            },
                            lantern: None,
                            processed_at: Utc::now(),
                            bridge_version: BRIDGE_VERSION.to_string(),
                            mirror_refresh: MirrorRefreshStatus::NotRequested,
                            error: Some(format!("Lantern supersede API call failed: {e}")),
                        };
                        let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                        return receipt;
                    }
                };

                if resp.status().is_success() {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let result_digest =
                        format!("sha256:{:x}", Sha256::digest(body.to_string().as_bytes()));

                    let receipt = Receipt {
                        schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                        intent_id: intent.intent_id.clone(),
                        intent_commit: discovered.commit_sha.clone(),
                        status: ReceiptStatus::Applied,
                        authority: ReceiptAuthority {
                            decision: "ALLOW".to_string(),
                            reason: None,
                        },
                        lantern: Some(ReceiptLantern {
                            record_id: Some(target_id.clone()),
                            result_sha256: Some(result_digest.clone()),
                        }),
                        processed_at: Utc::now(),
                        bridge_version: BRIDGE_VERSION.to_string(),
                        mirror_refresh: MirrorRefreshStatus::Requested,
                        error: None,
                    };
                    let _ = state.record_applied(
                        &intent.intent_id,
                        Some(&target_id),
                        Some(&result_digest),
                        receipt.clone(),
                    );
                    receipt
                } else {
                    let status_code = resp.status();
                    let err_text = resp.text().await.unwrap_or_default();
                    let receipt = Receipt {
                        schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                        intent_id: intent.intent_id.clone(),
                        intent_commit: discovered.commit_sha.clone(),
                        status: ReceiptStatus::Failed,
                        authority: ReceiptAuthority {
                            decision: "ALLOW".to_string(),
                            reason: None,
                        },
                        lantern: None,
                        processed_at: Utc::now(),
                        bridge_version: BRIDGE_VERSION.to_string(),
                        mirror_refresh: MirrorRefreshStatus::NotRequested,
                        error: Some(format!("Lantern HTTP {status_code}: {err_text}")),
                    };
                    let _ = state.record_terminal(&intent.intent_id, receipt.clone());
                    receipt
                }
            }
            IntentAction::MirrorRefresh => {
                let receipt = Receipt {
                    schema: super::schema::RECEIPT_SCHEMA_V1.to_string(),
                    intent_id: intent.intent_id.clone(),
                    intent_commit: discovered.commit_sha.clone(),
                    status: ReceiptStatus::Applied,
                    authority: ReceiptAuthority {
                        decision: "ALLOW".to_string(),
                        reason: None,
                    },
                    lantern: None,
                    processed_at: Utc::now(),
                    bridge_version: BRIDGE_VERSION.to_string(),
                    mirror_refresh: MirrorRefreshStatus::Requested,
                    error: None,
                };
                let _ = state.record_applied(&intent.intent_id, None, None, receipt.clone());
                receipt
            }
            IntentAction::MemoryReinforce => unreachable!(),
        }
    }

    async fn fetch_live_memory(&self, id: &str) -> Result<Option<serde_json::Value>, String> {
        let recall_url = format!("{}/api/v1/memories/recall", self.service_url);
        let recall_body = serde_json::json!({
            "include_inactive": true
        });

        let resp = self
            .client
            .post(&recall_url)
            .json(&recall_body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("Lantern HTTP {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(memories) = body.get("memories").and_then(|v| v.as_array()) {
            for mem in memories {
                if mem.get("id").and_then(|v| v.as_str()) == Some(id) {
                    return Ok(Some(mem.clone()));
                }
            }
        }

        Ok(None)
    }

    fn calculate_memory_digest(&self, memory: &serde_json::Value) -> String {
        let content = memory.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let kind = memory.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        hasher.update(b":");
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn check_observed_hash(
        &self,
        clean_obs: &str,
        record_id: &str,
        mem: &serde_json::Value,
    ) -> bool {
        // 1. Direct live content digest match
        let live_digest = self.calculate_memory_digest(mem);
        if clean_obs == live_digest || live_digest.contains(clean_obs) {
            return true;
        }

        // 2. Check mirror repository integrity sidecar file if available
        let mirror_dir = self.mirror_repo_path.clone().or_else(|| {
            std::env::var("LANTERN_BRIDGE_MIRROR_PATH")
                .ok()
                .map(PathBuf::from)
        });

        if let Some(mirror_base) = mirror_dir {
            let canonical = record_id.to_lowercase();
            let compact = canonical.replace('-', "");
            if compact.len() >= 4 {
                let candidate_paths = [
                    mirror_base
                        .join("mirror")
                        .join("records")
                        .join("memory")
                        .join("active")
                        .join(&compact[0..2])
                        .join(&compact[2..4])
                        .join(format!("{canonical}.integrity.json")),
                    mirror_base
                        .join("records")
                        .join("memory")
                        .join("active")
                        .join(&compact[0..2])
                        .join(&compact[2..4])
                        .join(format!("{canonical}.integrity.json")),
                ];
                for cand in &candidate_paths {
                    let matches = std::fs::read_to_string(cand)
                        .ok()
                        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                        .and_then(|val| {
                            val.get("sha256")
                                .and_then(|v| v.as_str())
                                .map(|h| h.eq_ignore_ascii_case(clean_obs))
                        })
                        .unwrap_or(false);
                    if matches {
                        return true;
                    }
                }
            }
        }

        false
    }
}
