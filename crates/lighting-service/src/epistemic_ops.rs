use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, BeliefLineage, BeliefRevision, BeliefRevisionId, BeliefState, Claim, ClaimId,
    ContextPack, ContextPackId, ContextTrace, DimensionDefinition, Episode, EpisodeTitle,
    EpistemicError, EpistemicRepository, EpistemicRepositoryError, MemoryItem, MemoryItemSearch,
    MemoryPathRepository, NewBelief, NewClaim, NewGraphRelation, NewMemoryItem, NewSource,
    PredicateDefinition, PredicateStatus, ReconciliationAction, ReconciliationDecision,
    SourceContent, SourceKind, SourceRepository, SourceTitle, Stance, StoreSourceResult, Trace,
    TraceId, TrustClass, normalize_registry_key, propagate_stale_beliefs, reconcile_claim,
};

use crate::epistemic_dto::{
    BeliefRequest, ClaimRequest, ContextCompileRequest, CorrectionRequest,
    DimensionDefinitionRequest, MemoryItemRequest, MemoryItemSearchRequest,
    PredicateDefinitionRequest, ReconcileClaimRequest, RelationRequest,
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
    #[error("epistemic repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, serde::Serialize)]
pub struct ReconciliationResult {
    pub claim: Claim,
    pub decision: ReconciliationDecision,
    pub belief: Option<Belief>,
}

#[derive(Debug, serde::Serialize)]
pub struct CorrectionResult {
    pub source_id: String,
    pub episode_id: String,
    pub claim: Claim,
    pub reconciliation: ReconciliationResult,
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

    pub async fn record_correction(
        &self,
        request: CorrectionRequest,
    ) -> Result<CorrectionResult, EpistemicOperationError> {
        let target_id =
            BeliefId::new(request.target_belief_id.trim().to_owned()).map_err(|_| {
                EpistemicOperationError::Invalid("target belief ID cannot be blank".to_owned())
            })?;
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
        let correction_text = required_text(request.correction_text, "correction text")?;
        let replacement_value = required_text(request.replacement_value, "replacement value")?;
        let (source_id, episode_id) = self.record_correction_evidence(&correction_text).await?;
        let now = Utc::now();
        let claim = Claim::new(NewClaim {
            episode_id: Some(episode_id.clone()),
            source_id: Some(source_id.clone()),
            evidence_span: Some((0, correction_text.len())),
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
        Ok(CorrectionResult {
            source_id,
            episode_id,
            claim,
            reconciliation,
        })
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
        let belief = self
            .repo
            .apply_belief_transition(previous, current, revisions, trace, stale_updates)
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
        self.repo
            .search_memory_items(&MemoryItemSearch {
                phrase,
                include_archived: request.include_archived,
                limit: request.limit,
            })
            .await
            .map_err(map_repository_error)
    }

    pub async fn compile_context(
        &self,
        request: ContextCompileRequest,
    ) -> Result<ContextPack, EpistemicOperationError> {
        let query = required_text(request.query, "context query")?;
        let item_budget = request.item_budget.clamp(1, 100);
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
        let query_tokens = context_tokens(&query);
        let mut belief_candidates: Vec<(i32, Belief)> = all_beliefs
            .into_iter()
            .map(|belief| (belief_relevance(&belief, &query_tokens, &scope), belief))
            .filter(|(score, belief)| *score > 0 || belief.stale)
            .collect();
        belief_candidates.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        let mut memory_candidates: Vec<(i32, MemoryItem)> = all_memories
            .into_iter()
            .map(|memory| (memory_relevance(&memory, &query_tokens), memory))
            .filter(|(score, _)| *score > 0)
            .collect();
        memory_candidates.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });

        let candidate_belief_ids = belief_candidates
            .iter()
            .map(|(_, belief)| belief.id.to_string())
            .collect::<Vec<_>>();
        let candidate_memory_ids = memory_candidates
            .iter()
            .map(|(_, memory)| memory.id.to_string())
            .collect::<Vec<_>>();
        let omitted_stale_ids = belief_candidates
            .iter()
            .filter(|(_, belief)| belief.stale)
            .map(|(_, belief)| belief.id.to_string())
            .collect::<Vec<_>>();

        let mut selected_beliefs = Vec::new();
        let mut selected_memories = Vec::new();
        for (_, belief) in belief_candidates {
            if selected_beliefs.len() + selected_memories.len() >= item_budget {
                break;
            }
            if !belief.stale {
                selected_beliefs.push(belief);
            }
        }
        for (_, memory) in memory_candidates {
            if selected_beliefs.len() + selected_memories.len() >= item_budget {
                break;
            }
            selected_memories.push(memory);
        }

        let pack_id = ContextPackId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
            EpistemicOperationError::Invalid("context ID could not be created".to_owned())
        })?;
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
            lanes: vec!["typed-belief".to_owned(), "lexical-soft-memory".to_owned()],
            item_budget,
        };
        let current_beliefs = selected_beliefs
            .iter()
            .filter(|belief| belief.holder_key == "matthew")
            .cloned()
            .collect::<Vec<_>>();
        let lucy_beliefs = selected_beliefs
            .iter()
            .filter(|belief| belief.holder_key == "lucy")
            .cloned()
            .collect::<Vec<_>>();
        let shared_beliefs = selected_beliefs
            .iter()
            .filter(|belief| belief.holder_key.starts_with("shared:"))
            .cloned()
            .collect::<Vec<_>>();
        let legacy_beliefs = selected_beliefs
            .iter()
            .filter(|belief| belief.holder_key.starts_with("legacy:"))
            .cloned()
            .collect::<Vec<_>>();
        let source_refs = selected_memories
            .iter()
            .filter_map(|memory| memory.source_id.clone())
            .collect::<Vec<_>>();
        let mut generated_context = render_context(
            &query,
            &current_beliefs,
            &lucy_beliefs,
            &shared_beliefs,
            &legacy_beliefs,
            &selected_memories,
            &omitted_stale_ids,
        );
        if let Some(intent) = request
            .intent
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            generated_context.insert_str(0, &format!("Intent: {}\n\n", intent.trim()));
        }
        let created_at = Utc::now();
        let pack = ContextPack {
            id: pack_id,
            query: query.clone(),
            selected_ids,
            omitted_stale_ids: omitted_stale_ids.clone(),
            generated_context,
            created_at,
            actor: request.actor,
            scope,
            current_beliefs,
            lucy_beliefs,
            shared_beliefs,
            legacy_beliefs,
            soft_memories: selected_memories,
            source_refs,
            retrieval_trace,
        };
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
            details: BTreeMap::from([
                ("compiler_version".to_owned(), "deterministic-v1".to_owned()),
                ("item_budget".to_owned(), item_budget.to_string()),
                ("query".to_owned(), query),
            ]),
            created_at,
        };
        self.repo
            .append_trace(trace)
            .await
            .map_err(map_repository_error)?;
        Ok(pack)
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

    async fn record_correction_evidence(
        &self,
        correction_text: &str,
    ) -> Result<(String, String), EpistemicOperationError> {
        let source_repo = self.source_repo.as_ref().ok_or_else(|| {
            EpistemicOperationError::Invalid(
                "correction evidence storage is not configured".to_owned(),
            )
        })?;
        let memory_path_repo = self.memory_path_repo.as_ref().ok_or_else(|| {
            EpistemicOperationError::Invalid(
                "correction evidence storage is not configured".to_owned(),
            )
        })?;
        let source = lighting_core::Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new("Matthew correction")
                .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?,
            content: SourceContent::new(correction_text.to_owned())
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
                    EpisodeTitle::new("Matthew correction")
                        .map_err(|error| EpistemicOperationError::Invalid(error.to_string()))?,
                    range,
                ))
                .await
                .map_err(|error| EpistemicOperationError::Repository(Box::new(error)))?
        };
        Ok((source_id.as_str().to_owned(), episode.id().to_string()))
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

fn context_tokens(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .map(str::to_ascii_lowercase)
        .collect()
}

fn belief_relevance(
    belief: &Belief,
    query_tokens: &[String],
    requested_scope: &lighting_core::Scope,
) -> i32 {
    let searchable = format!(
        "{} {} {} {}",
        belief.holder_key, belief.subject_key, belief.predicate_key, belief.current_value
    )
    .to_ascii_lowercase();
    let token_hits = query_tokens
        .iter()
        .filter(|token| searchable.contains(token.as_str()))
        .count() as i32;
    let scope_score = requested_scope
        .iter()
        .filter(|(key, value)| {
            belief
                .scope
                .get((*key).as_str())
                .is_some_and(|candidate| candidate == (*value).as_str())
        })
        .count() as i32
        * 3;
    token_hits + scope_score
}

fn memory_relevance(memory: &MemoryItem, query_tokens: &[String]) -> i32 {
    let searchable = memory.content.to_ascii_lowercase();
    query_tokens
        .iter()
        .filter(|token| searchable.contains(token.as_str()))
        .count() as i32
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
