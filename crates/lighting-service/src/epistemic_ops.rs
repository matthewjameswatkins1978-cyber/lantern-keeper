use std::sync::Arc;

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, Claim, DimensionDefinition, EpistemicError, EpistemicRepository,
    EpistemicRepositoryError, MemoryItem, MemoryItemSearch, NewBelief, NewClaim, NewGraphRelation,
    NewMemoryItem, PredicateDefinition, PredicateStatus, Stance, Trace, TraceId, TrustClass,
    normalize_registry_key,
};

use crate::epistemic_dto::{
    BeliefRequest, ClaimRequest, DimensionDefinitionRequest, MemoryItemRequest,
    MemoryItemSearchRequest, PredicateDefinitionRequest, RelationRequest,
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
        let claim = Claim::new(NewClaim {
            episode_id: request.episode_id,
            source_id: request.source_id,
            evidence_span: request.evidence_span,
            subject_key: request.subject_key,
            predicate_key: request.predicate_key,
            predicate_candidate: request.predicate_candidate,
            predicate_status: request
                .predicate_status
                .unwrap_or(PredicateStatus::Unmapped),
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
        let stored = self
            .repo
            .store_belief(belief)
            .await
            .map_err(map_repository_error)?;
        let trace = Trace {
            id: TraceId::new(uuid::Uuid::new_v4().to_string()).map_err(|_| {
                EpistemicOperationError::Invalid("trace ID could not be created".to_owned())
            })?,
            event_type: "belief_created".to_owned(),
            actor_id: stored.holder_key.clone(),
            subject_id: Some(stored.subject_key.clone()),
            input_ids: Vec::new(),
            output_ids: vec![stored.id.to_string()],
            details: Default::default(),
            created_at: now,
        };
        self.repo
            .append_trace(trace)
            .await
            .map_err(map_repository_error)?;
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
