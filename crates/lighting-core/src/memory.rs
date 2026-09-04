//! Durable, provenance-bearing Living Memory vocabulary.
//!
//! This is intentionally a small substrate rather than a complete ontology.
//! Source records remain authoritative evidence; Memory records are derived,
//! mutable understanding with explicit temporal and supersession fields.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{MemoryId, ProjectId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Fact,
    Decision,
    Preference,
    Instruction,
    Lesson,
    Gotcha,
    OpenLoop,
    Workflow,
    Summary,
    Entity,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Preference => "preference",
            Self::Instruction => "instruction",
            Self::Lesson => "lesson",
            Self::Gotcha => "gotcha",
            Self::OpenLoop => "open_loop",
            Self::Workflow => "workflow",
            Self::Summary => "summary",
            Self::Entity => "entity",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fact" => Some(Self::Fact),
            "decision" => Some(Self::Decision),
            "preference" => Some(Self::Preference),
            "instruction" => Some(Self::Instruction),
            "lesson" => Some(Self::Lesson),
            "gotcha" => Some(Self::Gotcha),
            "open_loop" => Some(Self::OpenLoop),
            "workflow" => Some(Self::Workflow),
            "summary" => Some(Self::Summary),
            "entity" => Some(Self::Entity),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Superseded,
    Archived,
}

impl MemoryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum MemoryError {
    #[error("memory content must not be blank")]
    BlankContent,
    #[error("memory confidence must be between 0 and 1")]
    InvalidConfidence,
    #[error("memory importance must be between 0 and 1")]
    InvalidImportance,
    #[error("memory agent must not be blank")]
    BlankAgent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: MemoryId,
    pub content: String,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub project_id: Option<ProjectId>,
    pub confidence: f32,
    pub importance: f32,
    pub recorded_at: DateTime<Utc>,
    pub known_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub derived_from: Vec<String>,
    pub supersedes: Vec<String>,
    pub contradicts: Vec<String>,
    pub supports: Vec<String>,
    pub agent: String,
}

#[derive(Clone, Debug)]
pub struct NewMemory {
    pub content: String,
    pub kind: MemoryKind,
    pub project_id: Option<ProjectId>,
    pub confidence: f32,
    pub importance: f32,
    pub recorded_at: DateTime<Utc>,
    pub known_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub derived_from: Vec<String>,
    pub supersedes: Vec<String>,
    pub contradicts: Vec<String>,
    pub supports: Vec<String>,
    pub agent: String,
}

impl Memory {
    pub fn new(input: NewMemory) -> Result<Self, MemoryError> {
        if input.content.trim().is_empty() {
            return Err(MemoryError::BlankContent);
        }
        if !(0.0..=1.0).contains(&input.confidence) {
            return Err(MemoryError::InvalidConfidence);
        }
        if !(0.0..=1.0).contains(&input.importance) {
            return Err(MemoryError::InvalidImportance);
        }
        if input.agent.trim().is_empty() {
            return Err(MemoryError::BlankAgent);
        }

        Ok(Self {
            id: MemoryId::new(uuid::Uuid::new_v4().to_string()).expect("UUID is never blank"),
            content: input.content,
            kind: input.kind,
            status: MemoryStatus::Active,
            project_id: input.project_id,
            confidence: input.confidence,
            importance: input.importance,
            recorded_at: input.recorded_at,
            known_at: input.known_at,
            valid_from: input.valid_from,
            valid_until: input.valid_until,
            superseded_at: None,
            derived_from: input.derived_from,
            supersedes: input.supersedes,
            contradicts: input.contradicts,
            supports: input.supports,
            agent: input.agent,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemorySearchQuery {
    pub project_id: Option<ProjectId>,
    pub phrase: Option<String>,
    pub include_inactive: bool,
}

#[derive(Debug, Error)]
pub enum MemoryRepositoryError {
    #[error("memory is unavailable")]
    Unavailable,
    #[error("memory repository operation failed: {0}")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("memory was not found")]
    NotFound,
}

#[async_trait::async_trait]
pub trait MemoryRepository: Send + Sync {
    async fn store(&self, memory: Memory) -> Result<Memory, MemoryRepositoryError>;
    async fn get(&self, id: &MemoryId) -> Result<Option<Memory>, MemoryRepositoryError>;
    async fn search(&self, query: &MemorySearchQuery)
    -> Result<Vec<Memory>, MemoryRepositoryError>;
    async fn supersede(
        &self,
        id: &MemoryId,
        at: DateTime<Utc>,
    ) -> Result<Memory, MemoryRepositoryError>;
}
