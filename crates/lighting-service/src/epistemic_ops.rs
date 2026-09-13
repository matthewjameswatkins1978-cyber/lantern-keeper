use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, BeliefLineage, BeliefRevision, BeliefRevisionId, BeliefState, Claim,
    DimensionDefinition, EpistemicError, EpistemicRepository, EpistemicRepositoryError, MemoryItem,
    MemoryItemSearch, NewBelief, NewClaim, NewGraphRelation, NewMemoryItem, PredicateDefinition,
    PredicateStatus, ReconciliationAction, ReconciliationDecision, Stance, Trace, TraceId,
    TrustClass, normalize_registry_key, reconcile_claim,
};

use crate::epistemic_dto::{
    BeliefRequest, ClaimRequest, DimensionDefinitionRequest, MemoryItemRequest,
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
    #[error("epistemic repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, serde::Serialize)]
pub struct ReconciliationResult {
    pub claim: Claim,
    pub decision: ReconciliationDecision,
    pub belief: Option<Belief>,
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
}

impl EpistemicService {
    pub fn new(repo: Arc<dyn EpistemicRepository>) -> Self {
        Self { repo }
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
        let existing = self
            .repo
            .list_beliefs(true)
            .await
            .map_err(map_repository_error)?
            .into_iter()
            .filter(|belief| {
                claim.predicate_key.as_ref() == Some(&belief.predicate_key)
                    && claim.subject_key == belief.subject_key
                    && claim.holder_actor_id.as_deref() == Some(belief.holder_key.as_str())
                    && claim.scope_hash == belief.scope_hash
                    && belief.state == BeliefState::Active
            })
            .max_by_key(|belief| belief.updated_at);
        let decision = reconcile_claim(&claim, existing.as_ref(), request.independent_evidence);
        let now = Utc::now();
        let trace_id = new_trace_id()?;
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
            .apply_belief_transition(previous, current, revisions, trace, Vec::new())
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
