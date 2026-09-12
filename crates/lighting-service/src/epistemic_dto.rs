use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use lighting_core::{
    Belief, Claim, DimensionDefinition, Frame, GraphRelation, MemoryItem, MemoryItemKind,
    PredicateDefinition, PredicateDefinitionStatus, PredicateStatus, Stance, TrustClass,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ClaimRequest {
    pub subject_key: String,
    pub value: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub evidence_span: Option<(usize, usize)>,
    #[serde(default)]
    pub predicate_key: Option<String>,
    #[serde(default)]
    pub predicate_candidate: Option<String>,
    #[serde(default)]
    pub predicate_status: Option<PredicateStatus>,
    #[serde(default)]
    pub scope: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub polarity: bool,
    pub originator_actor_id: String,
    pub speaker_actor_id: String,
    #[serde(default)]
    pub transmitter_actor_id: Option<String>,
    #[serde(default)]
    pub holder_actor_id: Option<String>,
    #[serde(default)]
    pub stance: Option<Stance>,
    #[serde(default)]
    pub framing_path: Vec<Frame>,
    #[serde(default = "default_claim_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub known_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_to: Option<DateTime<Utc>>,
    #[serde(default = "default_extractor")]
    pub extractor: String,
    #[serde(default = "default_version")]
    pub extractor_version: String,
}

#[derive(Debug, Deserialize)]
pub struct ClaimListQuery {
    #[serde(default)]
    pub unmapped: bool,
}

#[derive(Debug, Deserialize)]
pub struct BeliefRequest {
    pub holder_key: String,
    pub subject_key: String,
    pub predicate_key: String,
    pub current_value: String,
    #[serde(default)]
    pub scope: BTreeMap<String, String>,
    #[serde(default = "default_belief_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub trust_class: Option<TrustClass>,
    #[serde(default)]
    pub known_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct StaleBeliefRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryItemRequest {
    pub kind: MemoryItemKind,
    pub content: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub originator_actor_id: Option<String>,
    #[serde(default)]
    pub transmitter_actor_id: Option<String>,
    #[serde(default)]
    pub holder_actor_id: Option<String>,
    #[serde(default = "default_salience")]
    pub salience: f32,
}

#[derive(Debug, Deserialize)]
pub struct MemoryItemSearchRequest {
    #[serde(default)]
    pub phrase: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct RelationRequest {
    pub in_id: String,
    pub out_id: String,
    pub relation_type: String,
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default = "default_relation_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Debug, Deserialize)]
pub struct PredicateDefinitionRequest {
    pub key: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub value_type: String,
    #[serde(default)]
    pub allowed_dimensions: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_predicate_definition_status")]
    pub status: PredicateDefinitionStatus,
    #[serde(default = "default_actor")]
    pub actor_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DimensionDefinitionRequest {
    pub key: String,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default = "default_actor")]
    pub actor_id: String,
}

#[derive(Debug, Serialize)]
pub struct ClaimResponse {
    pub claim: Claim,
}

#[derive(Debug, Serialize)]
pub struct BeliefResponse {
    pub belief: Belief,
}

#[derive(Debug, Serialize)]
pub struct BeliefListResponse {
    pub beliefs: Vec<Belief>,
}

#[derive(Debug, Serialize)]
pub struct MemoryItemResponse {
    pub memory_item: MemoryItem,
}

#[derive(Debug, Serialize)]
pub struct MemoryItemListResponse {
    pub memory_items: Vec<MemoryItem>,
}

#[derive(Debug, Serialize)]
pub struct RelationResponse {
    pub relation: GraphRelation,
}

#[derive(Debug, Serialize)]
pub struct RelationListResponse {
    pub relations: Vec<GraphRelation>,
}

#[derive(Debug, Serialize)]
pub struct PredicateDefinitionResponse {
    pub predicate: PredicateDefinition,
}

#[derive(Debug, Serialize)]
pub struct PredicateDefinitionListResponse {
    pub predicates: Vec<PredicateDefinition>,
}

#[derive(Debug, Serialize)]
pub struct DimensionDefinitionResponse {
    pub dimension: DimensionDefinition,
}

#[derive(Debug, Serialize)]
pub struct DimensionDefinitionListResponse {
    pub dimensions: Vec<DimensionDefinition>,
}

fn default_true() -> bool {
    true
}
fn default_claim_confidence() -> f32 {
    0.5
}
fn default_belief_confidence() -> f32 {
    0.7
}
fn default_salience() -> f32 {
    0.5
}
fn default_limit() -> usize {
    50
}
fn default_extractor() -> String {
    "manual".to_owned()
}
fn default_version() -> String {
    "1".to_owned()
}
fn default_origin() -> String {
    "manual".to_owned()
}
fn default_relation_confidence() -> f32 {
    1.0
}
fn default_predicate_definition_status() -> PredicateDefinitionStatus {
    PredicateDefinitionStatus::Active
}
fn default_actor() -> String {
    "lucy".to_owned()
}
