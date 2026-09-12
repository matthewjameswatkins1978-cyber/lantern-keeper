//! Canonical living-memory records and epistemic safety vocabulary.
//!
//! Sources and Episodes preserve evidence.  Claims preserve what was asserted,
//! while Beliefs are mutable read projections.  Memory Items deliberately hold
//! useful material that is not ready to become a proposition.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{BeliefId, ClaimId, ContextPackId, MemoryItemId, ProposalId, TraceId};

pub type Scope = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameAction {
    Speaking,
    Quoting,
    Paraphrasing,
    Reporting,
    Echoing,
    Endorsing,
    Rejecting,
    Correcting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub actor_id: String,
    pub action: FrameAction,
    pub target_actor_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Neutral,
    Endorsing,
    Rejecting,
    Unobserved,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateStatus {
    Unmapped,
    Candidate,
    Resolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateDefinitionStatus {
    Active,
    Experimental,
    Deprecated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefState {
    Active,
    Disputed,
    Superseded,
    Archived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustClass {
    Direct,
    Derived,
    MigratedCanonical,
    Historical,
    Hypothesis,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryItemKind {
    Quote,
    Idea,
    Fragment,
    Impression,
    Anecdote,
    CreativeSeed,
    PatternCandidate,
    StrengthObservation,
    Lesson,
    OpenLoop,
    Tension,
    RejectedPath,
    NegativeConstraint,
    Reference,
    Humour,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Supports,
    Contradicts,
    Supersedes,
    DependsOn,
    DerivedFrom,
    Refines,
    Adopts,
    Rejects,
    RelatesTo,
    PartOf,
    About,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    pub display_name: String,
    pub metadata: BTreeMap<String, String>,
}

impl Actor {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, EpistemicError> {
        let id = non_blank(id.into(), "actor id")?;
        let display_name = non_blank(display_name.into(), "actor display name")?;
        Ok(Self {
            id,
            display_name,
            metadata: BTreeMap::new(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub episode_id: Option<String>,
    pub source_id: Option<String>,
    pub evidence_span: Option<(usize, usize)>,
    pub subject_key: String,
    pub predicate_key: Option<String>,
    pub predicate_candidate: Option<String>,
    pub predicate_status: PredicateStatus,
    pub value: String,
    pub scope: Scope,
    pub scope_hash: String,
    pub polarity: bool,
    pub originator_actor_id: String,
    pub speaker_actor_id: String,
    pub transmitter_actor_id: Option<String>,
    pub holder_actor_id: Option<String>,
    pub stance: Stance,
    pub framing_path: Vec<Frame>,
    pub confidence: f32,
    pub known_at: DateTime<Utc>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub extractor: String,
    pub extractor_version: String,
    pub created_at: DateTime<Utc>,
    pub dedupe_key: String,
}

impl Claim {
    pub fn new(input: NewClaim) -> Result<Self, EpistemicError> {
        validate_confidence(input.confidence)?;
        let subject_key = non_blank(input.subject_key, "claim subject")?;
        let value = non_blank(input.value, "claim value")?;
        let originator_actor_id = non_blank(input.originator_actor_id, "claim originator")?;
        let speaker_actor_id = non_blank(input.speaker_actor_id, "claim speaker")?;
        let extractor = non_blank(input.extractor, "claim extractor")?;
        let extractor_version = non_blank(input.extractor_version, "claim extractor version")?;
        let scope = normalize_scope(&input.scope)?;
        let scope_hash = scope_hash(&scope);
        let predicate_key = input
            .predicate_key
            .map(|value| normalize_registry_key(&value, "claim predicate"))
            .transpose()?;
        let predicate_candidate = input
            .predicate_candidate
            .map(|value| normalize_registry_key(&value, "claim predicate candidate"))
            .transpose()?;
        let dedupe_key = format!(
            "{}|{}|{}|{}",
            subject_key,
            predicate_key.as_deref().unwrap_or(""),
            scope_hash,
            value
        );
        Ok(Self {
            id: ClaimId::new(uuid::Uuid::new_v4().to_string())
                .map_err(|_| EpistemicError::Identifier)?,
            episode_id: input.episode_id,
            source_id: input.source_id,
            evidence_span: input.evidence_span,
            subject_key,
            predicate_key,
            predicate_candidate,
            predicate_status: input.predicate_status,
            value,
            scope,
            scope_hash,
            polarity: input.polarity,
            originator_actor_id,
            speaker_actor_id,
            transmitter_actor_id: input.transmitter_actor_id,
            holder_actor_id: input.holder_actor_id,
            stance: input.stance,
            framing_path: input.framing_path,
            confidence: input.confidence,
            known_at: input.known_at,
            valid_from: input.valid_from,
            valid_to: input.valid_to,
            extractor,
            extractor_version,
            created_at: input.created_at,
            dedupe_key,
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewClaim {
    pub episode_id: Option<String>,
    pub source_id: Option<String>,
    pub evidence_span: Option<(usize, usize)>,
    pub subject_key: String,
    pub predicate_key: Option<String>,
    pub predicate_candidate: Option<String>,
    pub predicate_status: PredicateStatus,
    pub value: String,
    pub scope: Scope,
    pub polarity: bool,
    pub originator_actor_id: String,
    pub speaker_actor_id: String,
    pub transmitter_actor_id: Option<String>,
    pub holder_actor_id: Option<String>,
    pub stance: Stance,
    pub framing_path: Vec<Frame>,
    pub confidence: f32,
    pub known_at: DateTime<Utc>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub extractor: String,
    pub extractor_version: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Belief {
    pub id: BeliefId,
    pub holder_key: String,
    pub subject_key: String,
    pub predicate_key: String,
    pub current_value: String,
    pub scope: Scope,
    pub scope_hash: String,
    pub state: BeliefState,
    pub confidence: f32,
    pub trust_class: TrustClass,
    pub known_from: DateTime<Utc>,
    pub known_to: Option<DateTime<Utc>>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub stale: bool,
    pub stale_since: Option<DateTime<Utc>>,
    pub stale_reason: Option<String>,
    pub dependency_generation: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Belief {
    pub fn new(input: NewBelief) -> Result<Self, EpistemicError> {
        validate_confidence(input.confidence)?;
        let scope = normalize_scope(&input.scope)?;
        Ok(Self {
            id: BeliefId::new(uuid::Uuid::new_v4().to_string())
                .map_err(|_| EpistemicError::Identifier)?,
            holder_key: non_blank(input.holder_key, "belief holder")?,
            subject_key: non_blank(input.subject_key, "belief subject")?,
            predicate_key: normalize_registry_key(&input.predicate_key, "belief predicate")?,
            current_value: non_blank(input.current_value, "belief value")?,
            scope_hash: scope_hash(&scope),
            scope,
            state: BeliefState::Active,
            confidence: input.confidence,
            trust_class: input.trust_class,
            known_from: input.known_from,
            known_to: None,
            valid_from: input.valid_from,
            valid_to: None,
            stale: false,
            stale_since: None,
            stale_reason: None,
            dependency_generation: 0,
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }

    /// Invalidate a dependent projection without changing its truth state.
    pub fn mark_stale(&mut self, reason: impl Into<String>, at: DateTime<Utc>) {
        self.stale = true;
        self.stale_since = Some(at);
        self.stale_reason = Some(reason.into());
        self.dependency_generation = self.dependency_generation.saturating_add(1);
        self.updated_at = at;
    }
}

#[derive(Clone, Debug)]
pub struct NewBelief {
    pub holder_key: String,
    pub subject_key: String,
    pub predicate_key: String,
    pub current_value: String,
    pub scope: Scope,
    pub confidence: f32,
    pub trust_class: TrustClass,
    pub known_from: DateTime<Utc>,
    pub valid_from: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: MemoryItemId,
    pub episode_id: Option<String>,
    pub source_id: Option<String>,
    pub kind: MemoryItemKind,
    pub content: String,
    pub originator_actor_id: Option<String>,
    pub transmitter_actor_id: Option<String>,
    pub holder_actor_id: Option<String>,
    pub salience: f32,
    pub reinforcement_count: u64,
    pub last_reinforced_at: Option<DateTime<Utc>>,
    pub pinned: bool,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub dedupe_key: String,
}

impl MemoryItem {
    pub fn new(input: NewMemoryItem) -> Result<Self, EpistemicError> {
        validate_unit(input.salience, "memory salience")?;
        let content = non_blank(input.content, "memory item content")?;
        let dedupe_key = format!("{:?}|{}", input.kind, content);
        Ok(Self {
            id: MemoryItemId::new(uuid::Uuid::new_v4().to_string())
                .map_err(|_| EpistemicError::Identifier)?,
            episode_id: input.episode_id,
            source_id: input.source_id,
            kind: input.kind,
            content,
            originator_actor_id: input.originator_actor_id,
            transmitter_actor_id: input.transmitter_actor_id,
            holder_actor_id: input.holder_actor_id,
            salience: input.salience,
            reinforcement_count: 0,
            last_reinforced_at: None,
            pinned: false,
            archived: false,
            created_at: input.created_at,
            dedupe_key,
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewMemoryItem {
    pub episode_id: Option<String>,
    pub source_id: Option<String>,
    pub kind: MemoryItemKind,
    pub content: String,
    pub originator_actor_id: Option<String>,
    pub transmitter_actor_id: Option<String>,
    pub holder_actor_id: Option<String>,
    pub salience: f32,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    pub id: TraceId,
    pub event_type: String,
    pub actor_id: String,
    pub subject_id: Option<String>,
    pub input_ids: Vec<String>,
    pub output_ids: Vec<String>,
    pub details: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    pub proposal_type: String,
    pub proposed_by: String,
    pub target_ids: Vec<String>,
    pub payload: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredicateDefinition {
    pub key: String,
    pub aliases: Vec<String>,
    pub value_type: String,
    pub allowed_dimensions: Vec<String>,
    pub description: String,
    pub status: PredicateDefinitionStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionDefinition {
    pub key: String,
    pub allowed_values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPack {
    pub id: ContextPackId,
    pub query: String,
    pub selected_ids: Vec<String>,
    pub omitted_stale_ids: Vec<String>,
    pub generated_context: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphRelation {
    pub id: crate::RelationId,
    pub in_id: String,
    pub out_id: String,
    pub relation_type: String,
    pub origin: String,
    pub confidence: f32,
    pub resolved: bool,
    pub created_at: DateTime<Utc>,
    pub dedupe_key: String,
}

#[derive(Clone, Debug)]
pub struct NewGraphRelation {
    pub in_id: String,
    pub out_id: String,
    pub relation_type: String,
    pub origin: String,
    pub confidence: f32,
    pub resolved: bool,
    pub created_at: DateTime<Utc>,
}

impl GraphRelation {
    pub fn new(input: NewGraphRelation) -> Result<Self, EpistemicError> {
        validate_confidence(input.confidence)?;
        let in_id = non_blank(input.in_id, "relation source")?;
        let out_id = non_blank(input.out_id, "relation target")?;
        let relation_type = non_blank(input.relation_type, "relation type")?;
        let origin = non_blank(input.origin, "relation origin")?;
        let dedupe_key = format!("{}|{}|{}", in_id, relation_type, out_id);
        Ok(Self {
            id: crate::RelationId::new(uuid::Uuid::new_v4().to_string())
                .map_err(|_| EpistemicError::Identifier)?,
            in_id,
            out_id,
            relation_type,
            origin,
            confidence: input.confidence,
            resolved: input.resolved,
            created_at: input.created_at,
            dedupe_key,
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EpistemicError {
    #[error("{0} must not be blank")]
    Blank(String),
    #[error("confidence must be between 0 and 1")]
    InvalidConfidence,
    #[error("{0} must be between 0 and 1")]
    InvalidUnit(String),
    #[error("invalid identifier")]
    Identifier,
    #[error("invalid scope: {0}")]
    InvalidScope(String),
}

#[derive(Debug, Error)]
pub enum EpistemicRepositoryError {
    #[error("epistemic repository operation failed: {0}")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("epistemic record was not found")]
    NotFound,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryItemSearch {
    pub phrase: Option<String>,
    pub include_archived: bool,
    pub limit: usize,
}

#[async_trait]
pub trait EpistemicRepository: Send + Sync {
    async fn store_claim(&self, claim: Claim) -> Result<Claim, EpistemicRepositoryError>;
    async fn get_claim(&self, id: &ClaimId) -> Result<Option<Claim>, EpistemicRepositoryError>;
    async fn store_belief(&self, belief: Belief) -> Result<Belief, EpistemicRepositoryError>;
    async fn get_belief(&self, id: &BeliefId) -> Result<Option<Belief>, EpistemicRepositoryError>;
    async fn list_beliefs(
        &self,
        include_stale: bool,
    ) -> Result<Vec<Belief>, EpistemicRepositoryError>;
    async fn mark_belief_stale(
        &self,
        id: &BeliefId,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Belief, EpistemicRepositoryError>;
    async fn store_memory_item(
        &self,
        item: MemoryItem,
    ) -> Result<MemoryItem, EpistemicRepositoryError>;
    async fn search_memory_items(
        &self,
        query: &MemoryItemSearch,
    ) -> Result<Vec<MemoryItem>, EpistemicRepositoryError>;
    async fn append_trace(&self, trace: Trace) -> Result<Trace, EpistemicRepositoryError>;
    async fn enqueue_proposal(
        &self,
        proposal: Proposal,
    ) -> Result<Proposal, EpistemicRepositoryError>;
    async fn store_relation(
        &self,
        relation: GraphRelation,
    ) -> Result<GraphRelation, EpistemicRepositoryError>;
    async fn list_unresolved_relations(
        &self,
    ) -> Result<Vec<GraphRelation>, EpistemicRepositoryError>;
    async fn store_predicate_definition(
        &self,
        definition: PredicateDefinition,
    ) -> Result<PredicateDefinition, EpistemicRepositoryError>;
    async fn get_predicate_definition(
        &self,
        key: &str,
    ) -> Result<Option<PredicateDefinition>, EpistemicRepositoryError>;
    async fn list_predicate_definitions(
        &self,
    ) -> Result<Vec<PredicateDefinition>, EpistemicRepositoryError>;
    async fn store_dimension_definition(
        &self,
        definition: DimensionDefinition,
    ) -> Result<DimensionDefinition, EpistemicRepositoryError>;
    async fn get_dimension_definition(
        &self,
        key: &str,
    ) -> Result<Option<DimensionDefinition>, EpistemicRepositoryError>;
    async fn list_dimension_definitions(
        &self,
    ) -> Result<Vec<DimensionDefinition>, EpistemicRepositoryError>;
}

pub fn scope_hash(scope: &Scope) -> String {
    use sha2::{Digest, Sha256};
    let mut entries = scope
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}\n",
                normalise_scope_key(key),
                normalise_scope_value(&normalise_scope_key(key), value)
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    let canonical = entries.concat();
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Return the canonical representation used by scoped Claims and Beliefs.
///
/// Keys are case-insensitive dimension names. Values are case-normalised only
/// for dimensions whose vocabulary is not an entity identifier; project and
/// device values retain their caller-provided casing after trimming.
pub fn normalize_scope(scope: &Scope) -> Result<Scope, EpistemicError> {
    let mut normalized = Scope::new();
    for (key, value) in scope {
        let normalized_key =
            non_blank(key.clone(), "scope key").map(|value| normalise_scope_key(&value))?;
        let normalized_value = non_blank(value.clone(), "scope value")
            .map(|value| normalise_scope_value(&normalized_key, &value))?;
        if normalized
            .insert(normalized_key.clone(), normalized_value)
            .is_some()
        {
            return Err(EpistemicError::InvalidScope(format!(
                "duplicate key after normalization: {normalized_key}"
            )));
        }
    }
    Ok(normalized)
}

pub fn normalize_registry_key(value: &str, field: &str) -> Result<String, EpistemicError> {
    let value = non_blank(value.to_owned(), field)?;
    let mut normalized = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator {
            normalized.push('_');
            separator = true;
        }
    }
    let normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        return Err(EpistemicError::Blank(field.to_owned()));
    }
    Ok(normalized)
}

fn normalise_scope_key(value: &str) -> String {
    normalize_registry_key(value, "scope key").unwrap_or_else(|_| value.trim().to_ascii_lowercase())
}

fn normalise_scope_value(key: &str, value: &str) -> String {
    let value = value.trim();
    match key {
        "os" => match value.to_ascii_lowercase().as_str() {
            "mac" | "macos" | "osx" => "macos".to_owned(),
            "win" | "windows" | "win32" => "windows".to_owned(),
            "linux" => "linux".to_owned(),
            _ => value.to_ascii_lowercase(),
        },
        "context" | "environment" | "location_context" | "activity" | "time_context" => {
            value.to_ascii_lowercase()
        }
        _ => value.to_owned(),
    }
}

fn non_blank(value: String, field: &str) -> Result<String, EpistemicError> {
    if value.trim().is_empty() {
        Err(EpistemicError::Blank(field.to_owned()))
    } else {
        Ok(value)
    }
}

fn validate_confidence(value: f32) -> Result<(), EpistemicError> {
    validate_unit(value, "confidence")
}

fn validate_unit(value: f32, field: &str) -> Result<(), EpistemicError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(EpistemicError::InvalidUnit(field.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_hash_is_order_independent() {
        let mut first = Scope::new();
        first.insert("context".to_owned(), "development".to_owned());
        first.insert("os".to_owned(), "macos".to_owned());
        let mut second = Scope::new();
        second.insert("os".to_owned(), "macos".to_owned());
        second.insert("context".to_owned(), "development".to_owned());
        assert_eq!(scope_hash(&first), scope_hash(&second));
    }

    #[test]
    fn scope_normalization_canonicalizes_keys_and_known_aliases() {
        let mut scope = Scope::new();
        scope.insert(" OS ".to_owned(), "OSX".to_owned());
        scope.insert("Context".to_owned(), "Development".to_owned());

        let normalized = normalize_scope(&scope).expect("scope is valid");

        assert_eq!(normalized.get("os"), Some(&"macos".to_owned()));
        assert_eq!(normalized.get("context"), Some(&"development".to_owned()));

        let mut equivalent = Scope::new();
        equivalent.insert("os".to_owned(), "mac".to_owned());
        equivalent.insert("context".to_owned(), "development".to_owned());
        assert_eq!(scope_hash(&scope), scope_hash(&equivalent));
    }

    #[test]
    fn scope_normalization_rejects_colliding_keys() {
        let mut scope = Scope::new();
        scope.insert("OS".to_owned(), "macos".to_owned());
        scope.insert("os".to_owned(), "windows".to_owned());

        assert!(matches!(
            normalize_scope(&scope),
            Err(EpistemicError::InvalidScope(message))
                if message.contains("duplicate key")
        ));
    }

    #[test]
    fn claim_normalizes_predicate_and_scope_before_identity() {
        let now = Utc::now();
        let mut scope = Scope::new();
        scope.insert("OS".to_owned(), "Mac".to_owned());
        let claim = Claim::new(NewClaim {
            episode_id: None,
            source_id: None,
            evidence_span: None,
            subject_key: "matthew".to_owned(),
            predicate_key: Some("Preferred Editor".to_owned()),
            predicate_candidate: None,
            predicate_status: PredicateStatus::Resolved,
            value: "Zed".to_owned(),
            scope,
            polarity: true,
            originator_actor_id: "matthew".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: None,
            holder_actor_id: Some("matthew".to_owned()),
            stance: Stance::Endorsing,
            framing_path: Vec::new(),
            confidence: 1.0,
            known_at: now,
            valid_from: None,
            valid_to: None,
            extractor: "test".to_owned(),
            extractor_version: "1".to_owned(),
            created_at: now,
        })
        .expect("claim is valid");

        assert_eq!(claim.predicate_key.as_deref(), Some("preferred_editor"));
        assert_eq!(claim.scope.get("os"), Some(&"macos".to_owned()));
    }

    #[test]
    fn belief_staleness_does_not_change_truth_state() {
        let now = Utc::now();
        let mut belief = Belief::new(NewBelief {
            holder_key: "lucy".to_owned(),
            subject_key: "matthew".to_owned(),
            predicate_key: "preferred_editor".to_owned(),
            current_value: "Zed".to_owned(),
            scope: Scope::new(),
            confidence: 0.9,
            trust_class: TrustClass::Hypothesis,
            known_from: now,
            valid_from: None,
            created_at: now,
        })
        .expect("test belief is valid");
        belief.mark_stale("upstream correction", now);
        assert!(belief.stale);
        assert_eq!(belief.state, BeliefState::Active);
        assert_eq!(belief.dependency_generation, 1);
    }

    #[test]
    fn claim_keeps_quote_and_transmission_separate() {
        let now = Utc::now();
        let claim = Claim::new(NewClaim {
            episode_id: None,
            source_id: None,
            evidence_span: None,
            subject_key: "matthew".to_owned(),
            predicate_key: None,
            predicate_candidate: Some("preferred_editor".to_owned()),
            predicate_status: PredicateStatus::Candidate,
            value: "Zed".to_owned(),
            scope: Scope::new(),
            polarity: true,
            originator_actor_id: "lucy".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: Some("matthew".to_owned()),
            holder_actor_id: Some("lucy".to_owned()),
            stance: Stance::Unobserved,
            framing_path: vec![Frame {
                actor_id: "matthew".to_owned(),
                action: FrameAction::Quoting,
                target_actor_id: Some("lucy".to_owned()),
            }],
            confidence: 0.4,
            known_at: now,
            valid_from: None,
            valid_to: None,
            extractor: "test".to_owned(),
            extractor_version: "1".to_owned(),
            created_at: now,
        })
        .expect("test claim is valid");
        assert_eq!(claim.originator_actor_id, "lucy");
        assert_eq!(claim.transmitter_actor_id.as_deref(), Some("matthew"));
        assert_eq!(claim.stance, Stance::Unobserved);
    }
}
