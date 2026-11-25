use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, BeliefLineage, BeliefRevision, BeliefRevisionId, BeliefState, Claim, ClaimId,
    ContextPack, ContextPackId, ContextSection, ContextTrace, CorrectionIdempotencyRecord,
    DimensionDefinition, Episode, EpisodeTitle, EpistemicError, EpistemicRepository,
    EpistemicRepositoryError, MemoryItem, MemoryItemSearch, MemoryPathRepository, NewBelief,
    NewClaim, NewGraphRelation, NewMemoryItem, NewSource, PredicateDefinition, PredicateStatus,
    Proposal, ProposalId, ReconciliationAction, ReconciliationDecision, Source, SourceContent,
    SourceKind, SourceRepository, SourceTitle, Stance, StoreSourceResult, Trace, TraceId,
    TrustClass, normalize_registry_key, propagate_stale_beliefs, reconcile_claim,
};
use sha2::{Digest, Sha256};

use crate::epistemic_dto::{
    BeliefRequest, ClaimRequest, ContextCompileRequest, CorrectionRequest,
    DimensionDefinitionRequest, ForemanReviewRequest, LiveClaimRequest, MemoryItemRequest,
    MemoryItemSearchRequest, PredicateDefinitionRequest, ReconcileClaimRequest, RelationRequest,
};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum EpistemicOperationError {
    #[error("epistemic repository is not available")]
    Unavailable,
    #[error("invalid epistemic request: {0}")]
    Invalid(String),
    #[error("epistemic record was not found")]
    NotFound,
    #[error("correction target revision does not match the expected current revision: {0}")]
    RevisionConflict(String),
    #[error("idempotency key was already used with a different correction payload: {0}")]
    IdempotencyConflict(String),
    #[error("epistemic repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReconciliationResult {
    pub claim: Claim,
    pub decision: ReconciliationDecision,
    pub belief: Option<Belief>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CorrectionResult {
    pub source_id: String,
    pub episode_id: String,
    pub context_pack_id: Option<String>,
    pub claim: Claim,
    pub reconciliation: ReconciliationResult,
}

#[derive(Debug, serde::Serialize)]
pub struct LiveClaimResult {
    pub source_id: String,
    pub episode_id: String,
    pub evidence_span: (usize, usize),
    pub claim: Claim,
    pub reconciliation: ReconciliationResult,
}

/// Hard cap on candidates fetched by `compile_context_scoped`.
///
/// LK-HOST-001 v1.1: <=128 total candidates across authorised lanes; the
/// hosted scoped path has a single typed-belief lane today.
const SCOPED_CANDIDATE_LIMIT: usize = 128;
/// Hard cap on unique lineage claim IDs hydrated per scoped context call.
/// Bounded point lookups; typed nonaccepted beyond. D1 batched hydration is
/// an M2 concern.
const MAX_LINEAGE_HYDRATION_IDS: usize = 512;
/// Response-size ceiling for hosted context packs (256 KiB, aligned with
/// `docs/mcp.md`).
const HOSTED_CONTEXT_PACK_MAX_BYTES: usize = 256 * 1024;
/// Principal bound into correction idempotency keys (delta-2). Matches the
/// existing correction originator; M3 replaces this with the authenticated
/// principal.
pub const CORRECTION_IDEMPOTENCY_PRINCIPAL: &str = "matthew";

struct TextEvidence {
    source_id: String,
    episode_id: String,
    evidence_span: (usize, usize),
}

#[derive(Debug, serde::Serialize)]
pub struct BeliefExplanation {
    pub belief: Belief,
    pub revisions: Vec<BeliefRevision>,
    pub supporting_claims: Vec<Claim>,
    pub contradictory_claims: Vec<Claim>,
}

impl From<EpistemicOperationError> for ApiError {
    fn from(error: EpistemicOperationError) -> Self {
        match error {
            EpistemicOperationError::Unavailable => ApiError::storage_unavailable(),
            EpistemicOperationError::Invalid(message) => ApiError {
                code: "invalid_epistemic_record".to_owned(),
                message,
            },
            EpistemicOperationError::NotFound => ApiError {
                code: "epistemic_record_not_found".to_owned(),
                message: "The requested epistemic record does not exist".to_owned(),
            },
            EpistemicOperationError::RevisionConflict(message) => ApiError {
                code: "revision_conflict".to_owned(),
                message,
            },
            EpistemicOperationError::IdempotencyConflict(message) => ApiError {
                code: "idempotency_conflict".to_owned(),
                message,
            },
            EpistemicOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct EpistemicService {
    repo: Arc<dyn EpistemicRepository>,
    source_repo: Option<Arc<dyn SourceRepository>>,
    memory_path_repo: Option<Arc<dyn MemoryPathRepository>>,
}

impl EpistemicService {
    pub fn new(repo: Arc<dyn EpistemicRepository>) -> Self {
        Self {
            repo,
            source_repo: None,
            memory_path_repo: None,
        }
    }

    pub fn new_with_evidence(
        repo: Arc<dyn EpistemicRepository>,
        source_repo: Arc<dyn SourceRepository>,
        memory_path_repo: Arc<dyn MemoryPathRepository>,
    ) -> Self {
        Self {
            repo,
            source_repo: Some(source_repo),
            memory_path_repo: Some(memory_path_repo),
        }
    }

    pub async fn capture_claim(
        &self,
        request: ClaimRequest,
    ) -> Result<Claim, EpistemicOperationError> {
        let now = Utc::now();
        let (predicate_key, predicate_candidate, predicate_status) = self
            .resolve_predicate(request.predicate_key, request.predicate_candidate)
            .await?;
        let claim = Claim::new(NewClaim {
            episode_id: request.episode_id,
            source_id: request.source_id,
            evidence_span: request.evidence_span,
            subject_key: request.subject_key,
            predicate_key,
            predicate_candidate,
            predicate_status,
            value: request.value,
            scope: request.scope,
            polarity: request.polarity,
            originator_actor_id: request.originator_actor_id,
            speaker_actor_id: request.speaker_actor_id,
            transmitter_actor_id: request.transmitter_actor_id,
            holder_actor_id: request.holder_actor_id,
            stance: request.stance.unwrap_or(Stance::Unobserved),
            framing_path: request.framing_path,
            confidence: request.confidence,
            known_at: request.known_at.unwrap_or(now),
            valid_from: request.valid_from,
            valid_to: request.valid_to,
            extractor: request.extractor,
            extractor_version: request.extractor_version,
            created_at: now,
        })
        .map_err(map_domain_error)?;
        self.repo
            .store_claim(claim)
            .await
            .map_err(map_repository_error)
    }

    pub async fn list_claims(
        &self,
        unmapped_only: bool,
    ) -> Result<Vec<Claim>, EpistemicOperationError> {
        self.repo
            .list_claims(unmapped_only)
            .await
            .map_err(map_repository_error)
    }

    /// Captures exact conversational evidence before creating and reconciling
    /// its factual Claim. Clients never need to manufacture provenance IDs.
    pub async fn capture_live_claim(
        &self,
        request: LiveClaimRequest,
    ) -> Result<LiveClaimResult, EpistemicOperationError> {
        if request.content.trim().is_empty() {
            return Err(EpistemicOperationError::Invalid(
                "claim content cannot be blank".to_owned(),
            ));
        }
        if request.subject_key.trim().is_empty() {
            return Err(EpistemicOperationError::Invalid(
                "claim subject_key cannot be blank".to_owned(),
            ));
        }
        let (predicate_key, predicate_candidate, predicate_status) = self
            .resolve_predicate(Some(request.predicate_key.clone()), None)
            .await?;
        let evidence = self
            .record_text_evidence("Lantern live factual capture", &request.evidence_text)
            .await?;
        let holder = request
            .holder_actor_id
            .clone()
            .unwrap_or_else(|| request.originator_actor_id.clone());
        let now = Utc::now();
        let claim = Claim::new(NewClaim {
            episode_id: Some(evidence.episode_id.clone()),
            source_id: Some(evidence.source_id.clone()),
            evidence_span: Some(evidence.evidence_span),
            subject_key: request.subject_key,
            predicate_key,
            predicate_candidate,
            predicate_status,
            value: request.content,
            scope: request.scope,
            polarity: true,
            originator_actor_id: request.originator_actor_id.clone(),
            speaker_actor_id: request.originator_actor_id,
            transmitter_actor_id: request.transmitter_actor_id,
            holder_actor_id: Some(holder),
            stance: Stance::Endorsing,
            framing_path: Vec::new(),
            confidence: request.confidence,
            known_at: now,
            valid_from: None,
            valid_to: None,
            extractor: "live_capture".to_owned(),
            extractor_version: "1".to_owned(),
            created_at: now,
        })
        .map_err(map_domain_error)?;
        let candidate_id = claim.id.clone();
        let claim = self
            .repo
            .store_claim(claim)
            .await
            .map_err(map_repository_error)?;
        let reconciliation = if claim.id != candidate_id {
            let belief = self
                .repo
                .list_beliefs(true)
                .await
                .map_err(map_repository_error)?
                .into_iter()
                .find(|belief| {
                    belief.state == BeliefState::Active
                        && belief
                            .lineage
                            .supporting_claim_ids
                            .iter()
                            .any(|id| id == claim.id.as_str())
                });
            ReconciliationResult {
                claim: claim.clone(),
                decision: ReconciliationDecision {
                    action: ReconciliationAction::NoChange,
                    reason: "identical live evidence was already captured".to_owned(),
                },
                belief,
            }
        } else {
            self.reconcile_claim(
                claim.id.as_str(),
                ReconcileClaimRequest {
                    independent_evidence: false,
                },
            )
            .await?
        };
        Ok(LiveClaimResult {
            source_id: evidence.source_id,
            episode_id: evidence.episode_id,
            evidence_span: evidence.evidence_span,
            claim,
            reconciliation,
        })
    }

    pub async fn record_correction(
        &self,
        request: CorrectionRequest,
    ) -> Result<CorrectionResult, EpistemicOperationError> {
        // Keyed idempotency envelope (LK-HOST-001 delta-2). With no key this
        // block is skipped entirely and the legacy path runs unchanged.
        let idempotency = match request.idempotency_key.as_deref() {
            None => None,
            Some(raw) => {
                let caller_key = raw.trim();
                if caller_key.is_empty() {
                    return Err(EpistemicOperationError::Invalid(
                        "idempotency key cannot be blank".to_owned(),
                    ));
                }
                if caller_key.contains('|') {
                    return Err(EpistemicOperationError::Invalid(
                        "idempotency key must not contain '|'".to_owned(),
                    ));
                }
                let composite = format!(
                    "{CORRECTION_IDEMPOTENCY_PRINCIPAL}|{}|{caller_key}",
                    lighting_core::scope_hash(&request.scope)
                );
                let payload_hash = correction_payload_hash(&request)?;
                if let Some(record) = self
                    .repo
                    .get_correction_idempotency(&composite)
                    .await
                    .map_err(map_repository_error)?
                {
                    if record.payload_hash != payload_hash {
                        return Err(EpistemicOperationError::IdempotencyConflict(
                            "idempotency key was already used with a different correction payload"
                                .to_owned(),
                        ));
                    }
                    let original: CorrectionResult = serde_json::from_str(&record.result_json)
                        .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?;
                    return Ok(original);
                }
                Some((composite, payload_hash))
            }
        };
        let target_id = match (
            request
                .target_belief_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            request
                .target_query
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        ) {
            (Some(_), Some(_)) => {
                return Err(EpistemicOperationError::Invalid(
                    "correction requires exactly one target: target_belief_id or target_query"
                        .to_owned(),
                ));
            }
            (Some(value), None) => BeliefId::new(value.to_owned()).map_err(|_| {
                EpistemicOperationError::Invalid("target belief ID cannot be blank".to_owned())
            })?,
            (None, Some(query)) => {
                let matches = self.search_beliefs(query, false).await?;
                if matches.len() != 1 {
                    return Err(EpistemicOperationError::Invalid(if matches.is_empty() {
                        "correction target query matched no active belief".to_owned()
                    } else {
                        format!(
                            "correction target query is ambiguous ({} active beliefs matched)",
                            matches.len()
                        )
                    }));
                }
                matches[0].id.clone()
            }
            (None, None) => {
                return Err(EpistemicOperationError::Invalid(
                    "correction requires exactly one target: target_belief_id or target_query"
                        .to_owned(),
                ));
            }
        };
        let target = self
            .repo
            .get_belief(&target_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)?;
        if target.state != BeliefState::Active {
            return Err(EpistemicOperationError::Invalid(
                "correction target must be an active belief".to_owned(),
            ));
        }
        let context_pack_id = request
            .context_pack_id
            .as_deref()
            .map(|value| {
                ContextPackId::new(value.trim().to_owned()).map_err(|_| {
                    EpistemicOperationError::Invalid("context pack ID cannot be blank".to_owned())
                })
            })
            .transpose()?;
        if let Some(pack_id) = context_pack_id.as_ref() {
            let pack = self
                .repo
                .get_context_pack(pack_id)
                .await
                .map_err(map_repository_error)?
                .ok_or(EpistemicOperationError::NotFound)?;
            if !pack.selected_ids.iter().any(|id| id == target.id.as_str()) {
                return Err(EpistemicOperationError::Invalid(
                    "context pack does not contain the correction target".to_owned(),
                ));
            }
        }
        // Expected-revision precondition (delta-2): verified after target
        // resolution and strictly before the first write, so a mismatch
        // persists nothing.
        if let Some(expected) = request.expected_revision.as_deref() {
            let actual = serde_json::to_value(target.updated_at)
                .map_err(|error| {
                    EpistemicOperationError::Invalid(format!(
                        "target revision could not be serialised: {error}"
                    ))
                })?
                .as_str()
                .map(str::to_owned)
                .unwrap_or_default();
            if actual != expected {
                return Err(EpistemicOperationError::RevisionConflict(format!(
                    "expected revision {expected}, stored revision is {actual}"
                )));
            }
        }
        let correction_text = request.correction_text;
        if correction_text.trim().is_empty() {
            return Err(EpistemicOperationError::Invalid(
                "correction text cannot be blank".to_owned(),
            ));
        }
        let replacement_value = required_text(request.replacement_value, "replacement value")?;
        let evidence = self
            .record_text_evidence("Matthew correction", &correction_text)
            .await?;
        let now = Utc::now();
        let claim = Claim::new(NewClaim {
            episode_id: Some(evidence.episode_id.clone()),
            source_id: Some(evidence.source_id.clone()),
            evidence_span: Some(evidence.evidence_span),
            subject_key: target.subject_key.clone(),
            predicate_key: Some(target.predicate_key.clone()),
            predicate_candidate: None,
            predicate_status: PredicateStatus::Resolved,
            value: replacement_value,
            scope: if request.scope.is_empty() {
                target.scope.clone()
            } else {
                request.scope
            },
            polarity: true,
            originator_actor_id: "matthew".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: None,
            holder_actor_id: Some(target.holder_key.clone()),
            stance: Stance::Endorsing,
            framing_path: Vec::new(),
            confidence: 1.0,
            known_at: now,
            valid_from: Some(now),
            valid_to: None,
            extractor: "matthew_correction".to_owned(),
            extractor_version: "1".to_owned(),
            created_at: now,
        })
        .map_err(map_domain_error)?;
        let claim = self
            .repo
            .store_claim(claim)
            .await
            .map_err(map_repository_error)?;
        let reconciliation = self
            .reconcile_claim(
                claim.id.as_str(),
                ReconcileClaimRequest {
                    independent_evidence: true,
                },
            )
            .await?;
        let result = CorrectionResult {
            source_id: evidence.source_id,
            episode_id: evidence.episode_id,
            context_pack_id: context_pack_id.map(|id| id.to_string()),
            claim,
            reconciliation,
        };
        if let Some((key, payload_hash)) = idempotency {
            let record = CorrectionIdempotencyRecord {
                key,
                payload_hash,
                result_json: serde_json::to_string(&result).map_err(|error| {
                    EpistemicOperationError::Invalid(format!(
                        "correction result could not be serialised: {error}"
                    ))
                })?,
                created_at: Utc::now(),
            };
            self.repo
                .store_correction_idempotency(record)
                .await
                .map_err(map_repository_error)?;
        }
        Ok(result)
    }

    pub async fn reconcile_claim(
        &self,
        id: &str,
        request: ReconcileClaimRequest,
    ) -> Result<ReconciliationResult, EpistemicOperationError> {
        let id = lighting_core::ClaimId::new(id.to_owned())
            .map_err(|_| EpistemicOperationError::Invalid("claim ID cannot be blank".to_owned()))?;
        let claim = self
            .repo
            .get_claim(&id)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)?;
        let all_beliefs = self
            .repo
            .list_beliefs(true)
            .await
            .map_err(map_repository_error)?;
        let existing = all_beliefs
            .iter()
            .filter(|belief| {
                claim.predicate_key.as_ref() == Some(&belief.predicate_key)
                    && claim.subject_key == belief.subject_key
                    && claim.holder_actor_id.as_deref() == Some(belief.holder_key.as_str())
                    && claim.scope_hash == belief.scope_hash
                    && belief.state == BeliefState::Active
            })
            .max_by_key(|belief| belief.updated_at)
            .cloned();
        let decision = reconcile_claim(&claim, existing.as_ref(), request.independent_evidence);
        let now = Utc::now();
        let trace_id = new_trace_id()?;
        let mut stale_updates = Vec::new();
        if let Some(upstream) = existing.as_ref()
            && matches!(
                decision.action,
                ReconciliationAction::Supersede | ReconciliationAction::Dispute
            )
        {
            let relations = self
                .repo
                .list_relations()
                .await
                .map_err(map_repository_error)?;
            let mut projections = all_beliefs.clone();
            let stale_ids = propagate_stale_beliefs(
                &mut projections,
                &relations,
                upstream.id.as_str(),
                "upstream belief changed",
                now,
            );
            stale_updates = projections
                .into_iter()
                .filter(|belief| stale_ids.contains(&belief.id.to_string()))
                .collect();
        }
        let mut previous = None;
        let mut current = None;
        let mut result_belief = existing.clone();
        let mut revisions = Vec::new();
        match decision.action {
            ReconciliationAction::Create => {
                let belief = belief_from_claim(&claim, now, None)?;
                revisions.push(make_revision(
                    &belief,
                    None,
                    Some(claim.value.clone()),
                    Some(&claim),
                    decision.action,
                    &trace_id,
                    now,
                )?);
                result_belief = Some(belief.clone());
                current = Some(belief);
            }
            ReconciliationAction::Reinforce => {
                let old = existing.clone().ok_or(EpistemicOperationError::NotFound)?;
                let mut belief = old.clone();
                push_unique(
                    &mut belief.lineage.supporting_claim_ids,
                    claim.id.to_string(),
                );
                belief.confidence = (belief.confidence + claim.confidence * 0.1).min(1.0);
                belief.stale = false;
                belief.stale_since = None;
                belief.stale_reason = None;
                belief.updated_at = now;
                revisions.push(make_revision(
                    &belief,
                    Some(old.current_value.clone()),
                    Some(belief.current_value.clone()),
                    Some(&claim),
                    decision.action,
                    &trace_id,
                    now,
                )?);
                previous = Some(old);
                result_belief = Some(belief.clone());
                current = Some(belief);
            }
            ReconciliationAction::Supersede => {
                let old = existing.clone().ok_or(EpistemicOperationError::NotFound)?;
                let mut historical = old.clone();
                historical.state = BeliefState::Superseded;
                historical.known_to = Some(claim.known_at);
                historical.updated_at = now;
                push_unique(
                    &mut historical.lineage.supporting_claim_ids,
                    claim.id.to_string(),
                );
                let replacement = belief_from_claim(&claim, now, Some(&old))?;
                revisions.push(make_revision(
                    &historical,
                    Some(old.current_value.clone()),
                    None,
                    Some(&claim),
                    decision.action,
                    &trace_id,
                    now,
                )?);
                revisions.push(make_revision(
                    &replacement,
                    Some(old.current_value.clone()),
                    Some(replacement.current_value.clone()),
                    Some(&claim),
                    decision.action,
                    &trace_id,
                    now,
                )?);
                previous = Some(historical);
                result_belief = Some(replacement.clone());
                current = Some(replacement);
            }
            ReconciliationAction::Contradict | ReconciliationAction::Dispute => {
                let old = existing.clone().ok_or(EpistemicOperationError::NotFound)?;
                let mut belief = old.clone();
                push_unique(
                    &mut belief.lineage.contradictory_claim_ids,
                    claim.id.to_string(),
                );
                if decision.action == ReconciliationAction::Dispute {
                    belief.state = BeliefState::Disputed;
                }
                belief.updated_at = now;
                revisions.push(make_revision(
                    &belief,
                    Some(old.current_value.clone()),
                    Some(belief.current_value.clone()),
                    Some(&claim),
                    decision.action,
                    &trace_id,
                    now,
                )?);
                previous = Some(old);
                result_belief = Some(belief.clone());
                current = Some(belief);
            }
            _ => {}
        }
        let mut details = BTreeMap::new();
        details.insert(
            "action".to_owned(),
            serde_json::to_string(&decision.action).unwrap_or_else(|_| "unknown".to_owned()),
        );
        details.insert("reason".to_owned(), decision.reason.clone());
        details.insert(
            "independent_evidence".to_owned(),
            request.independent_evidence.to_string(),
        );
        details.insert(
            "stale_dependency_count".to_owned(),
            stale_updates.len().to_string(),
        );
        if let Some(old) = previous.as_ref() {
            details.insert("old_value".to_owned(), old.current_value.clone());
        }
        if let Some(new) = current.as_ref() {
            details.insert("new_value".to_owned(), new.current_value.clone());
        }
        let mut input_ids = vec![claim.id.to_string()];
        if let Some(old) = previous.as_ref() {
            input_ids.push(old.id.to_string());
        }
        let trace = Trace {
            id: trace_id,
            event_type: "claim_reconciled".to_owned(),
            actor_id: "lucy".to_owned(),
            subject_id: Some(claim.subject_key.clone()),
            input_ids,
            output_ids: current
                .as_ref()
                .map(|belief| vec![belief.id.to_string()])
                .unwrap_or_default(),
            details,
            created_at: now,
        };
        // Q18 gap 3: the token this reconciliation READ is bound at the write,
        // so a concurrent commit between this read and the batch refuses the
        // whole transition instead of being overwritten.
        let expected_token = existing.as_ref().map(|belief| belief.updated_at);
        let belief = self
            .repo
            .apply_belief_transition_guarded(
                previous,
                current,
                revisions,
                trace,
                stale_updates,
                expected_token,
            )
            .await
            .map_err(map_repository_error)?;
        Ok(ReconciliationResult {
            claim,
            decision,
            belief: belief.or(result_belief),
        })
    }

    pub async fn create_belief(
        &self,
        request: BeliefRequest,
    ) -> Result<Belief, EpistemicOperationError> {
        let now = Utc::now();
        let belief = Belief::new(NewBelief {
            holder_key: request.holder_key,
            subject_key: request.subject_key,
            predicate_key: request.predicate_key,
            current_value: request.current_value,
            scope: request.scope,
            confidence: request.confidence,
            trust_class: request.trust_class.unwrap_or(TrustClass::Derived),
            known_from: request.known_from.unwrap_or(now),
            valid_from: request.valid_from,
            created_at: now,
        })
        .map_err(map_domain_error)?;
        let trace_id = new_trace_id()?;
        let trace = Trace {
            id: trace_id.clone(),
            event_type: "belief_created".to_owned(),
            actor_id: belief.holder_key.clone(),
            subject_id: Some(belief.subject_key.clone()),
            input_ids: Vec::new(),
            output_ids: vec![belief.id.to_string()],
            details: Default::default(),
            created_at: now,
        };
        let revision = make_revision(
            &belief,
            None,
            Some(belief.current_value.clone()),
            None,
            ReconciliationAction::Create,
            &trace_id,
            now,
        )?;
        let stored = self
            .repo
            .apply_belief_transition(None, Some(belief), vec![revision], trace, Vec::new())
            .await
            .map_err(map_repository_error)?
            .ok_or_else(|| {
                EpistemicOperationError::Invalid(
                    "belief transition did not produce a belief".to_owned(),
                )
            })?;
        Ok(stored)
    }

    pub async fn list_beliefs(
        &self,
        include_stale: bool,
    ) -> Result<Vec<Belief>, EpistemicOperationError> {
        self.repo
            .list_beliefs(include_stale)
            .await
            .map_err(map_repository_error)
    }

    pub async fn get_belief(&self, id: &str) -> Result<Belief, EpistemicOperationError> {
        let id = BeliefId::new(id.trim().to_owned()).map_err(|_| {
            EpistemicOperationError::Invalid("belief ID cannot be blank".to_owned())
        })?;
        self.repo
            .get_belief(&id)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)
    }

    pub async fn belief_history(
        &self,
        id: &str,
    ) -> Result<Vec<BeliefRevision>, EpistemicOperationError> {
        let belief = self.get_belief(id).await?;
        self.repo
            .list_belief_revisions(&belief.id)
            .await
            .map_err(map_repository_error)
    }

    pub async fn explain_belief(
        &self,
        id: &str,
    ) -> Result<BeliefExplanation, EpistemicOperationError> {
        let belief = self.get_belief(id).await?;
        let revisions = self
            .repo
            .list_belief_revisions(&belief.id)
            .await
            .map_err(map_repository_error)?;
        let claims = self
            .repo
            .list_claims(false)
            .await
            .map_err(map_repository_error)?;
        let supporting = belief
            .lineage
            .supporting_claim_ids
            .iter()
            .filter_map(|id| ClaimId::new(id.clone()).ok())
            .filter_map(|id| claims.iter().find(|claim| claim.id == id).cloned())
            .collect();
        let contradictory = belief
            .lineage
            .contradictory_claim_ids
            .iter()
            .filter_map(|id| ClaimId::new(id.clone()).ok())
            .filter_map(|id| claims.iter().find(|claim| claim.id == id).cloned())
            .collect();
        Ok(BeliefExplanation {
            belief,
            revisions,
            supporting_claims: supporting,
            contradictory_claims: contradictory,
        })
    }

    pub async fn search_beliefs(
        &self,
        query: &str,
        include_stale: bool,
    ) -> Result<Vec<Belief>, EpistemicOperationError> {
        let query = required_text(query.to_owned(), "belief search query")?;
        let tokens = context_tokens(&query);
        let mut results = self
            .repo
            .list_beliefs(include_stale)
            .await
            .map_err(map_repository_error)?
            .into_iter()
            .filter(|belief| include_stale || belief.state == BeliefState::Active)
            .map(|belief| (belief_relevance(&belief, &tokens, &BTreeMap::new()), belief))
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();
        results.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(results.into_iter().map(|(_, belief)| belief).collect())
    }

    pub async fn list_stale_beliefs(&self) -> Result<Vec<Belief>, EpistemicOperationError> {
        Ok(self
            .list_beliefs(true)
            .await?
            .into_iter()
            .filter(|belief| belief.stale)
            .collect())
    }

    pub async fn foreman_queue(
        &self,
        limit: usize,
    ) -> Result<Vec<Proposal>, EpistemicOperationError> {
        self.repo
            .list_proposals(limit.clamp(1, 50))
            .await
            .map_err(map_repository_error)
    }

    pub async fn foreman_review(
        &self,
        id: &str,
        request: ForemanReviewRequest,
    ) -> Result<Proposal, EpistemicOperationError> {
        let id = ProposalId::new(id.trim().to_owned()).map_err(|_| {
            EpistemicOperationError::Invalid("proposal ID cannot be blank".to_owned())
        })?;
        let mut proposal = self
            .repo
            .get_proposal(&id)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)?;
        let decision = request.decision.trim().to_ascii_lowercase();
        if !matches!(decision.as_str(), "accept" | "reject" | "modify" | "defer") {
            return Err(EpistemicOperationError::Invalid(
                "Foreman decision must be accept, reject, modify, or defer".to_owned(),
            ));
        }
        if proposal.status != "pending" {
            return Err(EpistemicOperationError::Invalid(
                "only pending proposals can be reviewed".to_owned(),
            ));
        }
        if decision == "modify" {
            proposal.payload = request
                .payload
                .filter(|payload| !payload.trim().is_empty())
                .ok_or_else(|| {
                    EpistemicOperationError::Invalid(
                        "modify requires a non-blank replacement payload".to_owned(),
                    )
                })?;
            proposal.decided_at = None;
        } else {
            proposal.status = match decision.as_str() {
                "accept" => "accepted",
                "reject" => "rejected",
                "defer" => "deferred",
                _ => unreachable!(),
            }
            .to_owned();
            proposal.decided_at = Some(Utc::now());
        }
        let proposal = self
            .repo
            .update_proposal(proposal)
            .await
            .map_err(map_repository_error)?;
        let trace = Trace {
            id: new_trace_id()?,
            event_type: "foreman_proposal_reviewed".to_owned(),
            actor_id: "lucy".to_owned(),
            subject_id: Some(proposal.id.to_string()),
            input_ids: vec![proposal.id.to_string()],
            output_ids: vec![proposal.id.to_string()],
            details: BTreeMap::from([
                ("decision".to_owned(), decision),
                ("status".to_owned(), proposal.status.clone()),
            ]),
            created_at: Utc::now(),
        };
        self.repo
            .append_trace(trace)
            .await
            .map_err(map_repository_error)?;
        Ok(proposal)
    }

    pub async fn mark_belief_stale(
        &self,
        id: &str,
        reason: String,
    ) -> Result<Belief, EpistemicOperationError> {
        let id = BeliefId::new(id.to_owned()).map_err(|_| {
            EpistemicOperationError::Invalid("belief ID cannot be blank".to_owned())
        })?;
        if reason.trim().is_empty() {
            return Err(EpistemicOperationError::Invalid(
                "stale reason cannot be blank".to_owned(),
            ));
        }
        self.repo
            .mark_belief_stale(&id, &reason, Utc::now())
            .await
            .map_err(map_repository_error)
    }

    pub async fn remember_soft(
        &self,
        request: MemoryItemRequest,
    ) -> Result<MemoryItem, EpistemicOperationError> {
        let item = MemoryItem::new(NewMemoryItem {
            episode_id: request.episode_id,
            source_id: request.source_id,
            kind: request.kind,
            content: request.content,
            originator_actor_id: request.originator_actor_id,
            transmitter_actor_id: request.transmitter_actor_id,
            holder_actor_id: request.holder_actor_id,
            salience: request.salience,
            created_at: Utc::now(),
        })
        .map_err(map_domain_error)?;
        self.repo
            .store_memory_item(item)
            .await
            .map_err(map_repository_error)
    }

    pub async fn search_soft(
        &self,
        request: MemoryItemSearchRequest,
    ) -> Result<Vec<MemoryItem>, EpistemicOperationError> {
        let phrase = request
            .phrase
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let Some(phrase) = phrase else {
            return self
                .repo
                .search_memory_items(&MemoryItemSearch {
                    phrase: None,
                    include_archived: request.include_archived,
                    limit: request.limit,
                })
                .await
                .map_err(map_repository_error);
        };
        let tokens = context_tokens(&phrase);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let memories = self
            .repo
            .search_memory_items(&MemoryItemSearch {
                phrase: None,
                include_archived: request.include_archived,
                limit: 0,
            })
            .await
            .map_err(map_repository_error)?;
        let mut matches = memories
            .into_iter()
            .filter_map(|memory| {
                let content = memory.content.to_ascii_lowercase();
                let score = tokens
                    .iter()
                    .filter(|token| content.contains(token.as_str()))
                    .count() as i32;
                (score > 0).then_some((score, memory))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| {
                    right
                        .salience
                        .partial_cmp(&left.salience)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        if request.limit > 0 {
            matches.truncate(request.limit);
        }
        Ok(matches.into_iter().map(|(_, memory)| memory).collect())
    }

    pub async fn compile_context(
        &self,
        request: ContextCompileRequest,
    ) -> Result<ContextPack, EpistemicOperationError> {
        let query = required_text(request.query, "context query")?;
        let item_budget = request.item_budget.clamp(1, 100);
        let token_budget = request.token_budget.clamp(128, 16_000);
        let scope = lighting_core::normalize_scope(&request.scope).map_err(map_domain_error)?;
        let all_beliefs = self
            .repo
            .list_beliefs(true)
            .await
            .map_err(map_repository_error)?;
        let all_memories = self
            .repo
            .search_memory_items(&MemoryItemSearch {
                phrase: None,
                include_archived: false,
                limit: 0,
            })
            .await
            .map_err(map_repository_error)?;
        let claims = self
            .repo
            .list_claims(false)
            .await
            .map_err(map_repository_error)?;
        let pack_id = ContextPackId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
            EpistemicOperationError::Invalid("context ID could not be created".to_owned())
        })?;
        let created_at = Utc::now();
        let selection = compile_select(CompileSelectInput {
            query,
            intent: request.intent,
            actor: request.actor,
            scope,
            project_hints: request.project_hints,
            item_budget,
            token_budget,
            pack_id,
            created_at,
            beliefs: all_beliefs,
            memories: all_memories,
            claims,
        });
        let mut pack = selection.pack;
        self.attach_sections(&mut pack).await?;
        self.repo
            .store_context_pack(pack.clone())
            .await
            .map_err(map_repository_error)?;
        let trace = Trace {
            id: TraceId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
                EpistemicOperationError::Invalid("trace ID could not be created".to_owned())
            })?,
            event_type: "context_compiled".to_owned(),
            actor_id: "lucy".to_owned(),
            subject_id: Some(pack.id.to_string()),
            input_ids: pack.selected_ids.clone(),
            output_ids: vec![pack.id.to_string()],
            details: selection.trace_details,
            created_at,
        };
        self.repo
            .append_trace(trace)
            .await
            .map_err(map_repository_error)?;
        Ok(pack)
    }

    /// Attaches SECTION-1 section hits to a freshly selected pack: bounded
    /// point lookups for the pack's evidence sources only, skipped when no
    /// source repository is wired (advisory, additive — never fails
    /// selection and never touches item/token budgets).
    async fn attach_sections(&self, pack: &mut ContextPack) -> Result<(), EpistemicOperationError> {
        let Some(source_repo) = self.source_repo.as_ref() else {
            return Ok(());
        };
        if pack.source_refs.is_empty() {
            return Ok(());
        }
        let mut sources = Vec::new();
        for reference in &pack.source_refs {
            let Some(id) = lighting_core::SourceId::parse(reference) else {
                continue;
            };
            if let Some(source) = source_repo
                .get(&id)
                .await
                .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
            {
                sources.push(source);
            }
        }
        let query_tokens = context_tokens(&pack.query);
        pack.sections = score_sections(&query_tokens, &sources);
        if !pack.sections.is_empty()
            && !pack
                .retrieval_trace
                .lanes
                .contains(&"section-overlap".to_owned())
        {
            pack.retrieval_trace
                .lanes
                .push("section-overlap".to_owned());
        }
        Ok(())
    }

    /// Compiles a bounded, zero-persistence context pack for the hosted
    /// profile (LK-HOST-001 M0 delta item 1, representative scoped call).
    ///
    /// Explicit, versioned differences from [`Self::compile_context`]:
    /// * candidates come from an indexed exact-scope-equality query capped at
    ///   `SCOPED_CANDIDATE_LIMIT` — never a fetch-all;
    /// * the soft-memory lane contributes no candidates: `MemoryItem` carries
    ///   no scope, so this path fails closed until M2 adds scoped soft memory;
    /// * lineage evidence for candidates is hydrated through bounded point
    ///   lookups, capped at `MAX_LINEAGE_HYDRATION_IDS` (typed nonaccepted beyond);
    /// * hosted budget caps apply: 32 items / 8,000 estimated tokens (v1.1 §5);
    /// * the pack is rejected above `HOSTED_CONTEXT_PACK_MAX_BYTES`;
    /// * **nothing is persisted** — no Context Pack row and no Trace (v1.1
    ///   zero-write read contract). Native `compile_context` keeps its
    ///   fetch-all and persistence behaviour unchanged.
    ///
    /// Exact scope equality fails closed: a broader authorised scope does not
    /// observe narrower-scoped records (M2 owns richer containment semantics).
    /// `request.scope` must be the authenticated principal's authorised
    /// scope; principal wiring is an M3 concern.
    pub async fn compile_context_scoped(
        &self,
        request: ContextCompileRequest,
    ) -> Result<ContextPack, EpistemicOperationError> {
        let query = required_text(request.query, "context query")?;
        let item_budget = request.item_budget.clamp(1, 32);
        let token_budget = request.token_budget.clamp(128, 8_000);
        let scope = lighting_core::normalize_scope(&request.scope).map_err(map_domain_error)?;
        let beliefs = self
            .repo
            .list_beliefs_scoped(&scope, true, SCOPED_CANDIDATE_LIMIT)
            .await
            .map_err(map_repository_error)?;
        let claims = self.hydrate_lineage_claims(&beliefs).await?;
        let pack_id = ContextPackId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
            EpistemicOperationError::Invalid("context ID could not be created".to_owned())
        })?;
        let mut pack = compile_select(CompileSelectInput {
            query,
            intent: request.intent,
            actor: request.actor,
            scope,
            project_hints: request.project_hints,
            item_budget,
            token_budget,
            pack_id,
            created_at: Utc::now(),
            beliefs,
            memories: Vec::new(),
            claims,
        })
        .pack;
        self.attach_sections(&mut pack).await?;
        let encoded = serde_json::to_string(&pack).map_err(|error| {
            EpistemicOperationError::Invalid(format!(
                "context pack could not be serialized: {error}"
            ))
        })?;
        if encoded.len() > HOSTED_CONTEXT_PACK_MAX_BYTES {
            return Err(EpistemicOperationError::Invalid(format!(
                "context pack exceeds the hosted serialization cap of {HOSTED_CONTEXT_PACK_MAX_BYTES} bytes"
            )));
        }
        Ok(pack)
    }

    /// Point-lookup hydration of the unique lineage claim IDs referenced by
    /// `beliefs`, bounded by `MAX_LINEAGE_HYDRATION_IDS` and failing closed
    /// beyond it. Missing claim records are tolerated, mirroring the native
    /// fetch-all path, which silently skips lineage it cannot find.
    async fn hydrate_lineage_claims(
        &self,
        beliefs: &[Belief],
    ) -> Result<Vec<Claim>, EpistemicOperationError> {
        let mut ids: Vec<ClaimId> = Vec::new();
        for belief in beliefs {
            for claim_id in belief
                .lineage
                .supporting_claim_ids
                .iter()
                .chain(belief.lineage.contradictory_claim_ids.iter())
            {
                let Ok(id) = ClaimId::new(claim_id.clone()) else {
                    continue;
                };
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        if ids.len() > MAX_LINEAGE_HYDRATION_IDS {
            return Err(EpistemicOperationError::Invalid(format!(
                "lineage hydration budget exceeded: {} unique claim ids (cap {MAX_LINEAGE_HYDRATION_IDS})",
                ids.len()
            )));
        }
        let mut claims = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(claim) = self
                .repo
                .get_claim(id)
                .await
                .map_err(map_repository_error)?
            {
                claims.push(claim);
            }
        }
        Ok(claims)
    }

    pub async fn store_relation(
        &self,
        request: RelationRequest,
    ) -> Result<lighting_core::GraphRelation, EpistemicOperationError> {
        let relation = lighting_core::GraphRelation::new(NewGraphRelation {
            in_id: request.in_id,
            out_id: request.out_id,
            relation_type: request.relation_type,
            origin: request.origin,
            confidence: request.confidence,
            resolved: request.resolved,
            created_at: Utc::now(),
        })
        .map_err(map_domain_error)?;
        self.repo
            .store_relation(relation)
            .await
            .map_err(map_repository_error)
    }

    pub async fn list_unresolved_relations(
        &self,
    ) -> Result<Vec<lighting_core::GraphRelation>, EpistemicOperationError> {
        self.repo
            .list_unresolved_relations()
            .await
            .map_err(map_repository_error)
    }

    pub async fn create_predicate_definition(
        &self,
        request: PredicateDefinitionRequest,
    ) -> Result<PredicateDefinition, EpistemicOperationError> {
        let key =
            normalize_registry_key(&request.key, "predicate key").map_err(map_domain_error)?;
        let value_type = required_text(request.value_type, "predicate value type")?;
        let aliases = normalize_unique_keys(request.aliases, "predicate alias")?
            .into_iter()
            .filter(|alias| alias != &key)
            .collect();
        let allowed_dimensions =
            normalize_unique_keys(request.allowed_dimensions, "predicate allowed dimension")?;
        if let Some(existing) = self
            .repo
            .get_predicate_definition(&key)
            .await
            .map_err(map_repository_error)?
        {
            return Ok(existing);
        }
        let definition = PredicateDefinition {
            key: key.clone(),
            aliases,
            value_type,
            allowed_dimensions,
            description: request.description.trim().to_owned(),
            status: request.status,
            created_at: Utc::now(),
        };
        let stored = self
            .repo
            .store_predicate_definition(definition)
            .await
            .map_err(map_repository_error)?;
        self.append_registry_trace(
            &request.actor_id,
            "predicate_definition_created",
            &stored.key,
        )
        .await?;
        Ok(stored)
    }

    pub async fn get_predicate_definition(
        &self,
        key: &str,
    ) -> Result<PredicateDefinition, EpistemicOperationError> {
        let key = normalize_registry_key(key, "predicate key").map_err(map_domain_error)?;
        self.repo
            .get_predicate_definition(&key)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)
    }

    pub async fn list_predicate_definitions(
        &self,
    ) -> Result<Vec<PredicateDefinition>, EpistemicOperationError> {
        self.repo
            .list_predicate_definitions()
            .await
            .map_err(map_repository_error)
    }

    pub async fn create_dimension_definition(
        &self,
        request: DimensionDefinitionRequest,
    ) -> Result<DimensionDefinition, EpistemicOperationError> {
        let key =
            normalize_registry_key(&request.key, "dimension key").map_err(map_domain_error)?;
        let allowed_values = normalize_unique_values(request.allowed_values);
        if let Some(existing) = self
            .repo
            .get_dimension_definition(&key)
            .await
            .map_err(map_repository_error)?
        {
            return Ok(existing);
        }
        let definition = DimensionDefinition {
            key: key.clone(),
            allowed_values,
        };
        let stored = self
            .repo
            .store_dimension_definition(definition)
            .await
            .map_err(map_repository_error)?;
        self.append_registry_trace(
            &request.actor_id,
            "dimension_definition_created",
            &stored.key,
        )
        .await?;
        Ok(stored)
    }

    pub async fn get_dimension_definition(
        &self,
        key: &str,
    ) -> Result<DimensionDefinition, EpistemicOperationError> {
        let key = normalize_registry_key(key, "dimension key").map_err(map_domain_error)?;
        self.repo
            .get_dimension_definition(&key)
            .await
            .map_err(map_repository_error)?
            .ok_or(EpistemicOperationError::NotFound)
    }

    pub async fn list_dimension_definitions(
        &self,
    ) -> Result<Vec<DimensionDefinition>, EpistemicOperationError> {
        self.repo
            .list_dimension_definitions()
            .await
            .map_err(map_repository_error)
    }

    async fn resolve_predicate(
        &self,
        predicate_key: Option<String>,
        predicate_candidate: Option<String>,
    ) -> Result<(Option<String>, Option<String>, PredicateStatus), EpistemicOperationError> {
        let definitions = self
            .repo
            .list_predicate_definitions()
            .await
            .map_err(map_repository_error)?;
        let supplied = predicate_key.or(predicate_candidate);
        let Some(supplied) = supplied else {
            return Ok((None, None, PredicateStatus::Unmapped));
        };
        let normalized =
            normalize_registry_key(&supplied, "predicate candidate").map_err(map_domain_error)?;
        if let Some(definition) = definitions.iter().find(|definition| {
            definition.key == normalized
                || definition.aliases.iter().any(|alias| alias == &normalized)
        }) {
            return Ok((
                Some(definition.key.clone()),
                None,
                PredicateStatus::Resolved,
            ));
        }
        Ok((None, Some(normalized), PredicateStatus::Unmapped))
    }

    async fn record_text_evidence(
        &self,
        title: &str,
        evidence_text: &str,
    ) -> Result<TextEvidence, EpistemicOperationError> {
        if evidence_text.trim().is_empty() {
            return Err(EpistemicOperationError::Invalid(
                "evidence text cannot be blank".to_owned(),
            ));
        }
        let source_repo = self.source_repo.as_ref().ok_or_else(|| {
            EpistemicOperationError::Invalid("text evidence storage is not configured".to_owned())
        })?;
        let memory_path_repo = self.memory_path_repo.as_ref().ok_or_else(|| {
            EpistemicOperationError::Invalid("text evidence storage is not configured".to_owned())
        })?;
        let source = lighting_core::Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new(title)
                .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?,
            content: SourceContent::new(evidence_text.to_owned())
                .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?,
        });
        let source_id = match source_repo
            .store(source)
            .await
            .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
        {
            StoreSourceResult::Stored(source) => source.id().clone(),
            StoreSourceResult::Duplicate { existing_id, .. } => existing_id,
        };
        let source = source_repo
            .get(&source_id)
            .await
            .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
            .ok_or_else(|| {
                EpistemicOperationError::Invalid(
                    "stored correction Source could not be reloaded".to_owned(),
                )
            })?;
        let end_byte = source.content().as_bytes().len();
        let episode = if let Some(existing) = memory_path_repo
            .find_episode_by_source_range(&source_id, 0, end_byte)
            .await
            .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
        {
            existing
        } else {
            let range =
                lighting_core::SourceRange::new(source_id.clone(), 0, end_byte, source.content())
                    .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?;
            memory_path_repo
                .create_episode(Episode::new(
                    EpisodeTitle::new(title)
                        .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?,
                    range,
                ))
                .await
                .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
        };
        Ok(TextEvidence {
            source_id: source_id.as_str().to_owned(),
            episode_id: episode.id().to_string(),
            evidence_span: (0, end_byte),
        })
    }

    async fn append_registry_trace(
        &self,
        actor_id: &str,
        event_type: &str,
        key: &str,
    ) -> Result<(), EpistemicOperationError> {
        let actor_id = required_text(actor_id.to_owned(), "registry actor")?;
        let trace = Trace {
            id: TraceId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
                EpistemicOperationError::Invalid("trace ID could not be created".to_owned())
            })?,
            event_type: event_type.to_owned(),
            actor_id,
            subject_id: Some(key.to_owned()),
            input_ids: Vec::new(),
            output_ids: vec![key.to_owned()],
            details: Default::default(),
            created_at: Utc::now(),
        };
        self.repo
            .append_trace(trace)
            .await
            .map_err(map_repository_error)?;
        Ok(())
    }
}

/// Pure selection phase of context compilation: candidates in, pack out.
///
/// This is the portable seam half of [`EpistemicService::compile_context`]
/// (LK-HOST-001 v1.1 zero-write read contract). It performs no repository
/// access, no clock reads and no random ID generation: `pack_id` and
/// `created_at` are supplied by the caller, and persistence of the pack and
/// its Trace stays with the native caller.
struct CompileSelectInput {
    query: String,
    intent: Option<String>,
    actor: Option<String>,
    scope: lighting_core::Scope,
    project_hints: Vec<String>,
    item_budget: usize,
    token_budget: usize,
    pack_id: ContextPackId,
    created_at: chrono::DateTime<Utc>,
    beliefs: Vec<Belief>,
    memories: Vec<MemoryItem>,
    claims: Vec<Claim>,
}

struct CompileSelectOutput {
    pack: ContextPack,
    /// Exact `Trace.details` payload for the native caller to persist.
    trace_details: BTreeMap<String, String>,
}

fn compile_select(input: CompileSelectInput) -> CompileSelectOutput {
    let CompileSelectInput {
        query,
        intent,
        actor,
        scope,
        project_hints,
        item_budget,
        token_budget,
        pack_id,
        created_at,
        beliefs: all_beliefs,
        memories: all_memories,
        claims,
    } = input;
    let mut searchable_query = query.clone();
    for hint in &project_hints {
        searchable_query.push(' ');
        searchable_query.push_str(hint);
    }
    let query_tokens = context_tokens(&searchable_query);
    // State×intent (SECTION-1): history intent in the query is what makes
    // history-state records first-class candidates for this selection — a
    // superseded record gets a +3 intent bonus instead of the active +2.
    let history_intent = history_requested(&query_tokens);
    let mut belief_candidates: Vec<(i32, Vec<String>, Belief)> = all_beliefs
        .into_iter()
        .map(|belief| {
            let state_bonus = match belief.state {
                BeliefState::Active => 2,
                _ if history_intent => 3,
                _ => 0,
            };
            let (relevance, mut reasons) = belief_relevance_details(&belief, &query_tokens, &scope);
            match belief.state {
                BeliefState::Active => reasons.push("active-current-projection".to_owned()),
                _ if history_intent => {
                    reasons.push("history-intent-history-state-boost".to_owned());
                }
                _ => reasons.push("historical-or-inactive-projection".to_owned()),
            }
            (relevance + state_bonus, reasons, belief)
        })
        .filter(|(score, _, belief)| {
            belief.stale
                || (belief.state == BeliefState::Active && *score > 2)
                || (belief.state != BeliefState::Active && *score > 0)
        })
        .collect();
    belief_candidates.sort_by(|(left_score, _, left), (right_score, _, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    let mut memory_candidates: Vec<(i32, Vec<String>, MemoryItem)> = all_memories
        .into_iter()
        .map(|memory| {
            let (score, reasons) = memory_relevance_details(&memory, &query_tokens);
            (score, reasons, memory)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();
    memory_candidates.sort_by(|(left_score, _, left), (right_score, _, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });

    let candidate_belief_ids = belief_candidates
        .iter()
        .map(|(_, _, belief)| belief.id.to_string())
        .collect::<Vec<_>>();
    let candidate_memory_ids = memory_candidates
        .iter()
        .map(|(_, _, memory)| memory.id.to_string())
        .collect::<Vec<_>>();
    let mut candidate_scores = std::collections::BTreeMap::new();
    let mut candidate_reasons = std::collections::BTreeMap::new();
    for (score, reasons, belief) in &belief_candidates {
        candidate_scores.insert(belief.id.to_string(), *score);
        candidate_reasons.insert(belief.id.to_string(), reasons.clone());
    }
    for (score, reasons, memory) in &memory_candidates {
        candidate_scores.insert(memory.id.to_string(), *score);
        candidate_reasons.insert(memory.id.to_string(), reasons.clone());
    }
    let omitted_stale_ids = belief_candidates
        .iter()
        .filter(|(_, _, belief)| belief.stale)
        .map(|(_, _, belief)| belief.id.to_string())
        .collect::<Vec<_>>();

    let mut selected_beliefs = Vec::new();
    let mut selected_memories = Vec::new();
    let mut estimated_token_usage =
        estimate_tokens(&format!("# Lantern Context\n\nQuery: {query}\n"));
    for (_, _, belief) in belief_candidates {
        if selected_beliefs.len() + selected_memories.len() >= item_budget {
            break;
        }
        let item_tokens = estimate_belief_tokens(&belief);
        let eligible = belief.state == BeliefState::Active
            || history_intent
            || historical_value_requested(&belief, &query_tokens);
        if eligible
            && !belief.stale
            && estimated_token_usage.saturating_add(item_tokens) <= token_budget
        {
            selected_beliefs.push(belief);
            estimated_token_usage = estimated_token_usage.saturating_add(item_tokens);
        }
    }
    for (_, _, memory) in memory_candidates {
        if selected_beliefs.len() + selected_memories.len() >= item_budget {
            break;
        }
        let item_tokens = estimate_memory_tokens(&memory);
        if estimated_token_usage.saturating_add(item_tokens) <= token_budget {
            selected_memories.push(memory);
            estimated_token_usage = estimated_token_usage.saturating_add(item_tokens);
        }
    }

    let selected_ids = selected_beliefs
        .iter()
        .map(|belief| belief.id.to_string())
        .chain(selected_memories.iter().map(|memory| memory.id.to_string()))
        .collect::<Vec<_>>();
    let retrieval_trace = ContextTrace {
        compiler_version: "deterministic-v1".to_owned(),
        candidate_belief_ids,
        candidate_memory_ids,
        selected_ids: selected_ids.clone(),
        excluded_stale_ids: omitted_stale_ids.clone(),
        lanes: {
            let mut lanes = vec!["typed-belief".to_owned(), "lexical-soft-memory".to_owned()];
            if !project_hints.is_empty() {
                lanes.push("project-hint".to_owned());
            }
            if history_intent {
                lanes.push("temporal-history".to_owned());
            }
            lanes
        },
        candidate_scores,
        candidate_reasons,
        item_budget,
        token_budget,
        estimated_token_usage,
    };
    let current_beliefs = selected_beliefs
        .iter()
        .filter(|belief| belief.state == BeliefState::Active && belief.holder_key == "matthew")
        .cloned()
        .collect::<Vec<_>>();
    let historical_beliefs = selected_beliefs
        .iter()
        .filter(|belief| belief.state != BeliefState::Active)
        .cloned()
        .collect::<Vec<_>>();
    let lucy_beliefs = selected_beliefs
        .iter()
        .filter(|belief| belief.state == BeliefState::Active && belief.holder_key == "lucy")
        .cloned()
        .collect::<Vec<_>>();
    let shared_beliefs = selected_beliefs
        .iter()
        .filter(|belief| {
            belief.state == BeliefState::Active && belief.holder_key.starts_with("shared:")
        })
        .cloned()
        .collect::<Vec<_>>();
    let legacy_beliefs = selected_beliefs
        .iter()
        .filter(|belief| {
            belief.state == BeliefState::Active && belief.holder_key.starts_with("legacy:")
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut source_refs = Vec::new();
    let mut episode_refs = Vec::new();
    for memory in &selected_memories {
        if let Some(source_id) = &memory.source_id {
            push_unique(&mut source_refs, source_id.clone());
        }
        if let Some(episode_id) = &memory.episode_id {
            push_unique(&mut episode_refs, episode_id.clone());
        }
    }
    for belief in &selected_beliefs {
        for claim_id in belief
            .lineage
            .supporting_claim_ids
            .iter()
            .chain(belief.lineage.contradictory_claim_ids.iter())
        {
            if let Some(claim) = claims.iter().find(|claim| claim.id.as_str() == claim_id) {
                if let Some(source_id) = &claim.source_id {
                    push_unique(&mut source_refs, source_id.clone());
                }
                if let Some(episode_id) = &claim.episode_id {
                    push_unique(&mut episode_refs, episode_id.clone());
                }
            }
        }
    }
    let mut generated_context = render_context(
        &query,
        &current_beliefs,
        &lucy_beliefs,
        &shared_beliefs,
        &legacy_beliefs,
        &selected_memories,
        &omitted_stale_ids,
    );
    insert_historical_beliefs(&mut generated_context, &historical_beliefs);
    if let Some(intent) = intent.as_deref().filter(|value| !value.trim().is_empty()) {
        generated_context.insert_str(0, &format!("Intent: {}\n\n", intent.trim()));
    }
    let pack = ContextPack {
        id: pack_id,
        query: query.clone(),
        selected_ids,
        omitted_stale_ids: omitted_stale_ids.clone(),
        generated_context,
        created_at,
        actor,
        scope,
        current_beliefs,
        historical_beliefs,
        lucy_beliefs,
        shared_beliefs,
        legacy_beliefs,
        soft_memories: selected_memories,
        source_refs,
        episode_refs,
        sections: Vec::new(),
        retrieval_trace,
    };
    let trace_details = BTreeMap::from([
        ("compiler_version".to_owned(), "deterministic-v1".to_owned()),
        ("item_budget".to_owned(), item_budget.to_string()),
        ("token_budget".to_owned(), token_budget.to_string()),
        (
            "estimated_token_usage".to_owned(),
            estimated_token_usage.to_string(),
        ),
        ("query".to_owned(), query),
    ]);
    CompileSelectOutput {
        pack,
        trace_details,
    }
}

fn belief_from_claim(
    claim: &Claim,
    now: chrono::DateTime<Utc>,
    prior: Option<&Belief>,
) -> Result<Belief, EpistemicOperationError> {
    let holder = claim.holder_actor_id.clone().ok_or_else(|| {
        EpistemicOperationError::Invalid("reconciliation requires a holder actor".to_owned())
    })?;
    let predicate = claim.predicate_key.clone().ok_or_else(|| {
        EpistemicOperationError::Invalid("reconciliation requires a resolved predicate".to_owned())
    })?;
    let mut belief = Belief::new(NewBelief {
        holder_key: holder.clone(),
        subject_key: claim.subject_key.clone(),
        predicate_key: predicate,
        current_value: claim.value.clone(),
        scope: claim.scope.clone(),
        confidence: claim.confidence,
        trust_class: if holder == claim.originator_actor_id {
            TrustClass::Direct
        } else {
            TrustClass::Derived
        },
        known_from: claim.known_at,
        valid_from: claim.valid_from,
        created_at: now,
    })
    .map_err(map_domain_error)?;
    belief.valid_to = claim.valid_to;
    belief.lineage = BeliefLineage {
        supporting_claim_ids: vec![claim.id.to_string()],
        contradictory_claim_ids: Vec::new(),
        prior_belief_id: prior.map(|belief| belief.id.to_string()),
        derived_from_belief_ids: Vec::new(),
    };
    Ok(belief)
}

/// Canonical payload hash for a keyed correction envelope (delta-2): binds
/// the request's target selection, text, value, scope, pack and expected
/// revision so a reused key with a changed envelope conflicts instead of
/// replaying. `serde_json` object maps are sorted, so the encoding is
/// deterministic.
fn correction_payload_hash(request: &CorrectionRequest) -> Result<String, EpistemicOperationError> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "target_belief_id": request.target_belief_id,
        "target_query": request.target_query,
        "correction_text": request.correction_text,
        "replacement_value": request.replacement_value,
        "scope": request.scope,
        "context_pack_id": request.context_pack_id,
        "expected_revision": request.expected_revision,
    }))
    .map_err(|error| {
        EpistemicOperationError::Invalid(format!(
            "correction payload could not be canonicalised: {error}"
        ))
    })?;
    let digest = Sha256::digest(canonical);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Maximum section hits attached to one pack (SECTION-1; additive,
/// budget-exempt — this is a presentation bound, not a retrieval budget).
const MAX_SECTION_HITS: usize = 10;

/// SECTION-1 section-aware scorer: parses each source's outline with
/// [`lighting_core::parse_outline`] and ranks its sections by query-token
/// overlap. Pure and deterministic: score desc, then source id, then start
/// byte; sections with no overlap are dropped; the result is capped at
/// [`MAX_SECTION_HITS`].
fn score_sections(query_tokens: &[String], sources: &[Source]) -> Vec<ContextSection> {
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for source in sources {
        let content = source.content().as_str();
        let outline = lighting_core::parse_outline(content);
        for heading in outline.headings {
            let section_text = &content[heading.start_byte..heading.end_byte];
            let section_tokens = context_tokens(section_text);
            let score = query_tokens
                .iter()
                .filter(|token| section_tokens.iter().any(|candidate| candidate == *token))
                .count() as i32;
            if score > 0 {
                hits.push(ContextSection {
                    source_id: source.id().as_str().to_owned(),
                    level: heading.level,
                    title: heading.title,
                    start_byte: heading.start_byte,
                    end_byte: heading.end_byte,
                    score,
                });
            }
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            // Specificity: a parent section's byte range contains its
            // children's, so on equal overlap the narrower section ranks
            // first.
            .then_with(|| {
                (left.end_byte - left.start_byte).cmp(&(right.end_byte - right.start_byte))
            })
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.start_byte.cmp(&right.start_byte))
    });
    hits.truncate(MAX_SECTION_HITS);
    hits
}

fn context_tokens(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .map(str::to_ascii_lowercase)
        .filter(|token| !is_lexical_stopword(token))
        .collect()
}

fn is_lexical_stopword(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "but"
            | "by"
            | "do"
            | "does"
            | "for"
            | "from"
            | "how"
            | "i"
            | "if"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "which"
            | "who"
            | "why"
            | "with"
            | "you"
    )
}

fn history_requested(query_tokens: &[String]) -> bool {
    query_tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "history"
                | "historical"
                | "used"
                | "formerly"
                | "previous"
                | "previously"
                | "old"
                | "before"
                | "changed"
                | "change"
                | "replaced"
                | "replacement"
        )
    })
}

fn historical_value_requested(belief: &Belief, query_tokens: &[String]) -> bool {
    let value_tokens = context_tokens(&belief.current_value);
    value_tokens.iter().any(|value_token| {
        query_tokens
            .iter()
            .any(|query_token| query_token == value_token)
    })
}

fn belief_relevance(
    belief: &Belief,
    query_tokens: &[String],
    requested_scope: &lighting_core::Scope,
) -> i32 {
    belief_relevance_details(belief, query_tokens, requested_scope).0
}

fn belief_relevance_details(
    belief: &Belief,
    query_tokens: &[String],
    requested_scope: &lighting_core::Scope,
) -> (i32, Vec<String>) {
    let searchable = format!(
        "{} {} {} {}",
        belief.holder_key, belief.subject_key, belief.predicate_key, belief.current_value
    );
    let searchable_tokens = context_tokens(&searchable);
    let token_hits = query_tokens
        .iter()
        .filter(|token| !is_generic_belief_identity(token))
        .filter(|token| {
            searchable_tokens
                .iter()
                .any(|candidate| candidate == *token)
        })
        .count() as i32;
    let scope_matches = requested_scope
        .iter()
        .filter(|(key, value)| {
            belief
                .scope
                .get((*key).as_str())
                .is_some_and(|candidate| candidate == (*value).as_str())
        })
        .count() as i32;
    let mut reasons = Vec::new();
    if token_hits > 0 {
        reasons.push(format!("lexical-match:{token_hits}"));
    }
    if scope_matches > 0 {
        reasons.push(format!("scope-match:{scope_matches}"));
    }
    (token_hits + scope_matches * 3, reasons)
}

fn is_generic_belief_identity(token: &str) -> bool {
    matches!(token, "matthew" | "lucy" | "shared" | "legacy")
}

fn memory_relevance_details(memory: &MemoryItem, query_tokens: &[String]) -> (i32, Vec<String>) {
    let searchable_tokens = context_tokens(&memory.content);
    let token_hits = query_tokens
        .iter()
        .filter(|token| {
            searchable_tokens
                .iter()
                .any(|candidate| candidate == *token)
        })
        .count() as i32;
    let mut reasons = Vec::new();
    if token_hits > 0 {
        reasons.push(format!("lexical-match:{token_hits}"));
    }
    if memory.pinned {
        reasons.push("explicitly-pinned".to_owned());
    }
    if memory.salience >= 0.75 {
        reasons.push("high-salience".to_owned());
    }
    (token_hits, reasons)
}

fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4).max(1)
}

fn estimate_belief_tokens(belief: &Belief) -> usize {
    estimate_tokens(&format!(
        "- [belief {}] {}.{} = {} (confidence {:.2}, scope {:?})\n",
        belief.id,
        belief.subject_key,
        belief.predicate_key,
        belief.current_value,
        belief.confidence,
        belief.scope
    ))
}

fn estimate_memory_tokens(memory: &MemoryItem) -> usize {
    estimate_tokens(&format!(
        "- [memory {}] {:?}: {}\n",
        memory.id, memory.kind, memory.content
    ))
}

fn render_context(
    query: &str,
    current_beliefs: &[Belief],
    lucy_beliefs: &[Belief],
    shared_beliefs: &[Belief],
    legacy_beliefs: &[Belief],
    soft_memories: &[MemoryItem],
    omitted_stale_ids: &[String],
) -> String {
    let mut rendered = format!("# Lantern Context\n\nQuery: {query}\n");
    rendered.push('\n');
    render_belief_section(&mut rendered, "Matthew beliefs", current_beliefs);
    render_belief_section(&mut rendered, "Lucy beliefs", lucy_beliefs);
    render_belief_section(&mut rendered, "Shared beliefs", shared_beliefs);
    render_belief_section(&mut rendered, "Legacy beliefs", legacy_beliefs);
    rendered.push_str("## Soft memories\n");
    if soft_memories.is_empty() {
        rendered.push_str("None selected.\n\n");
    } else {
        for memory in soft_memories {
            rendered.push_str(&format!(
                "- [memory {}] {:?}: {}\n",
                memory.id, memory.kind, memory.content
            ));
        }
        rendered.push('\n');
    }
    rendered.push_str("## Stale exclusions\n");
    if omitted_stale_ids.is_empty() {
        rendered.push_str("None.\n");
    } else {
        for id in omitted_stale_ids {
            rendered.push_str("- ");
            rendered.push_str(id);
            rendered.push_str(" (not injected as current context)\n");
        }
    }
    rendered
}

fn insert_historical_beliefs(rendered: &mut String, beliefs: &[Belief]) {
    let mut section = String::new();
    render_belief_section(&mut section, "Historical beliefs", beliefs);
    let marker = "## Stale exclusions\n";
    if let Some(index) = rendered.find(marker) {
        rendered.insert_str(index, &section);
    } else {
        rendered.push_str(&section);
    }
}

fn render_belief_section(rendered: &mut String, title: &str, beliefs: &[Belief]) {
    rendered.push_str("## ");
    rendered.push_str(title);
    rendered.push('\n');
    if beliefs.is_empty() {
        rendered.push_str("None selected.\n\n");
        return;
    }
    for belief in beliefs {
        rendered.push_str(&format!(
            "- [belief {}] {}.{} = {} (confidence {:.2}, scope {:?})\n",
            belief.id,
            belief.subject_key,
            belief.predicate_key,
            belief.current_value,
            belief.confidence,
            belief.scope
        ));
    }
    rendered.push('\n');
}

fn make_revision(
    belief: &Belief,
    previous_value: Option<String>,
    new_value: Option<String>,
    claim: Option<&Claim>,
    transition_type: ReconciliationAction,
    trace_id: &TraceId,
    created_at: chrono::DateTime<Utc>,
) -> Result<BeliefRevision, EpistemicOperationError> {
    Ok(BeliefRevision {
        id: BeliefRevisionId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
            EpistemicOperationError::Invalid("belief revision ID could not be created".to_owned())
        })?,
        belief_id: belief.id.clone(),
        previous_value,
        new_value,
        claim_id: claim.map(|claim| claim.id.clone()),
        transition_type,
        trace_id: trace_id.clone(),
        created_at,
    })
}

fn new_trace_id() -> Result<TraceId, EpistemicOperationError> {
    TraceId::new(uuid::Uuid::new_v4().to_string())
        .map_err(|_| EpistemicOperationError::Invalid("trace ID could not be created".to_owned()))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn required_text(value: String, field: &str) -> Result<String, EpistemicOperationError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        Err(EpistemicOperationError::Invalid(format!(
            "{field} cannot be blank"
        )))
    } else {
        Ok(value)
    }
}

fn normalize_unique_keys(
    values: Vec<String>,
    field: &str,
) -> Result<Vec<String>, EpistemicOperationError> {
    let mut result = Vec::new();
    for value in values {
        let value = normalize_registry_key(&value, field)
            .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?;
        if !result.contains(&value) {
            result.push(value);
        }
    }
    Ok(result)
}

fn normalize_unique_values(values: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        let value = value.trim().to_owned();
        if !value.is_empty() && !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

fn map_domain_error(error: EpistemicError) -> EpistemicOperationError {
    EpistemicOperationError::Invalid(error.to_string())
}

fn map_repository_error(error: EpistemicRepositoryError) -> EpistemicOperationError {
    match error {
        EpistemicRepositoryError::NotFound => EpistemicOperationError::NotFound,
        EpistemicRepositoryError::Operation(error) => EpistemicOperationError::Repository(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_belief_is_not_selected_for_holder_only_query() {
        let now = Utc::now();
        let belief = Belief::new(NewBelief {
            holder_key: "matthew".to_owned(),
            subject_key: "matthew".to_owned(),
            predicate_key: "connected_client_test_colour".to_owned(),
            current_value: "green".to_owned(),
            scope: BTreeMap::new(),
            confidence: 1.0,
            trust_class: TrustClass::Direct,
            known_from: now,
            valid_from: Some(now),
            created_at: now,
        })
        .expect("test belief is valid");
        let tokens = context_tokens("How do Matthew and Lucy work together?");

        assert_eq!(belief_relevance(&belief, &tokens, &BTreeMap::new()), 0);
    }

    fn ranking_belief(state: BeliefState, value: &str, at: chrono::DateTime<Utc>) -> Belief {
        let mut belief = Belief::new(NewBelief {
            holder_key: "matthew".to_owned(),
            subject_key: "bench.editor".to_owned(),
            predicate_key: "preferred".to_owned(),
            current_value: value.to_owned(),
            scope: BTreeMap::from([("project".to_owned(), "bench".to_owned())]),
            confidence: 0.9,
            trust_class: TrustClass::Direct,
            known_from: at,
            valid_from: Some(at),
            created_at: at,
        })
        .expect("test belief is valid");
        belief.updated_at = at;
        belief.state = state;
        belief
    }

    fn compile_ranking(query: &str, beliefs: Vec<Belief>) -> CompileSelectOutput {
        compile_select(CompileSelectInput {
            query: query.to_owned(),
            intent: None,
            actor: Some("test".to_owned()),
            scope: BTreeMap::new(),
            project_hints: Vec::new(),
            item_budget: 10,
            token_budget: 2048,
            pack_id: ContextPackId::new("pack-section-test".to_owned()).expect("pack id"),
            created_at: Utc::now(),
            beliefs,
            memories: Vec::new(),
            claims: Vec::new(),
        })
    }

    #[test]
    fn history_intent_boosts_history_state_records_into_selection() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(5);
        let active = ranking_belief(BeliefState::Active, "current pick alpha pair1", now);
        let superseded = ranking_belief(BeliefState::Superseded, "legacy-entry-old1", earlier);
        let decoy = ranking_belief(BeliefState::Active, "unrelated value", earlier);
        let active_id = active.id.to_string();
        let superseded_id = superseded.id.to_string();
        let decoy_id = decoy.id.to_string();

        let output = compile_ranking(
            "history editor preferred pair1",
            vec![active, superseded, decoy],
        );

        assert!(
            output.pack.selected_ids.contains(&active_id),
            "active projection must stay selected under history intent"
        );
        assert!(
            output.pack.selected_ids.contains(&superseded_id),
            "history intent must surface the superseded record"
        );
        let reasons = output
            .pack
            .retrieval_trace
            .candidate_reasons
            .get(&superseded_id)
            .expect("superseded candidate reasons recorded");
        assert!(
            reasons
                .iter()
                .any(|reason| reason == "history-intent-history-state-boost"),
            "boost reason must be recorded: {reasons:?}"
        );
        let scores = &output.pack.retrieval_trace.candidate_scores;
        assert!(
            scores[&superseded_id] > scores[&decoy_id],
            "boosted superseded record must outrank the scope-only active decoy ({} vs {})",
            scores[&superseded_id],
            scores[&decoy_id]
        );
        assert!(
            output
                .pack
                .retrieval_trace
                .lanes
                .contains(&"temporal-history".to_owned()),
            "history lane must be recorded"
        );
    }

    #[test]
    fn without_history_intent_history_state_records_stay_unselected() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(5);
        let active = ranking_belief(BeliefState::Active, "current pick alpha pair1", now);
        let superseded = ranking_belief(BeliefState::Superseded, "legacy-entry-old1", earlier);
        let active_id = active.id.to_string();
        let superseded_id = superseded.id.to_string();

        let output = compile_ranking("editor preferred pair1", vec![active, superseded]);

        assert!(
            output.pack.selected_ids.contains(&active_id),
            "active projection must be selected for the ordinary query"
        );
        assert!(
            !output.pack.selected_ids.contains(&superseded_id),
            "superseded record must NOT surface without history intent"
        );
        let reasons = output
            .pack
            .retrieval_trace
            .candidate_reasons
            .get(&superseded_id)
            .expect("superseded candidate reasons recorded");
        assert!(
            !reasons
                .iter()
                .any(|reason| reason == "history-intent-history-state-boost"),
            "no boost without history intent: {reasons:?}"
        );
    }

    #[test]
    fn section_scorer_ranks_overlapping_sections_with_byte_ranges() {
        let source = Source::create(NewSource {
            kind: SourceKind::Markdown,
            title: SourceTitle::new("bench guide").expect("title"),
            content: SourceContent::new(
                "# Setup\n\nInstall the fixture pack.\n\n## Configuration\n\nEdit the runtime flags.\n\n## Teardown\n\nStop the helper services.\n"
                    .to_owned(),
            )
            .expect("content"),
        });
        let tokens = context_tokens("configuration runtime flags");
        let hits = score_sections(&tokens, std::slice::from_ref(&source));

        assert!(!hits.is_empty(), "Configuration section must be a hit");
        assert_eq!(hits[0].title, "Configuration");
        assert!(hits[0].score >= 2);
        let content = source.content().as_str();
        let section = &content[hits[0].start_byte..hits[0].end_byte];
        assert!(
            section.starts_with("## Configuration"),
            "byte range must slice the real section"
        );
        // The parent Setup section's byte range contains its children's, so it
        // also overlaps the query; the narrower Configuration section must
        // still rank first (specificity tie-break).
        let titles: Vec<&str> = hits.iter().map(|hit| hit.title.as_str()).collect();
        let config_at = titles
            .iter()
            .position(|title| *title == "Configuration")
            .expect("Configuration must be a hit");
        let setup_at = titles
            .iter()
            .position(|title| *title == "Setup")
            .expect("Setup spans the query tokens as the parent section");
        assert!(
            config_at < setup_at,
            "specific section must outrank its parent: {titles:?}"
        );
        assert!(
            score_sections(&[], std::slice::from_ref(&source)).is_empty(),
            "empty query must yield no sections"
        );
    }

    #[test]
    fn section_scorer_is_capped_and_deterministic() {
        let mut content = String::new();
        for index in 0..15 {
            content.push_str(&format!("# Shared topic {index}\n\ntopic body\n"));
        }
        let source = Source::create(NewSource {
            kind: SourceKind::Markdown,
            title: SourceTitle::new("long note").expect("title"),
            content: SourceContent::new(content).expect("content"),
        });
        let tokens = context_tokens("topic");
        let first = score_sections(&tokens, std::slice::from_ref(&source));
        let second = score_sections(&tokens, std::slice::from_ref(&source));
        assert_eq!(first.len(), MAX_SECTION_HITS, "cap must hold");
        assert_eq!(first, second, "scoring must be deterministic");
    }
}
