use chrono::{DateTime, Utc};
use lighting_core::{Memory, MemoryKind, MemoryStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RememberRequest {
    pub content: String,
    pub kind: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default = "default_importance")]
    pub importance: f32,
    #[serde(default)]
    pub derived_from: Vec<String>,
    #[serde(default)]
    pub updates: Vec<String>,
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub contradicts: Vec<String>,
    #[serde(default)]
    pub supports: Vec<String>,
    #[serde(default = "default_agent")]
    pub agent: String,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
}

fn default_confidence() -> f32 {
    0.8
}

fn default_importance() -> f32 {
    0.5
}

fn default_agent() -> String {
    "lucy".to_owned()
}

#[derive(Debug, Serialize)]
pub struct MemoryResponse {
    pub memory: Memory,
}

#[derive(Debug, Deserialize)]
pub struct RecallRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub phrase: Option<String>,
    #[serde(default)]
    pub include_inactive: bool,
    #[serde(default)]
    pub as_of: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct RetrievalTrace {
    pub query: Option<String>,
    pub candidate_memory_ids: Vec<String>,
    pub selected_memory_ids: Vec<String>,
    pub channel: String,
}

#[derive(Debug, Serialize)]
pub struct RecallResponse {
    pub memories: Vec<Memory>,
    pub abstained: bool,
    pub reason: Option<String>,
    pub trace: RetrievalTrace,
}

#[derive(Debug, Deserialize)]
pub struct ContextRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ContextResponse {
    pub context: String,
    pub memories: Vec<Memory>,
    pub abstained: bool,
    pub reason: Option<String>,
    pub trace: RetrievalTrace,
}

#[derive(Debug, Serialize)]
pub struct SupersedeResponse {
    pub memory: Memory,
    pub status: MemoryStatus,
    pub superseded_at: Option<DateTime<Utc>>,
}

#[allow(dead_code)]
fn _kind_is_serializable(kind: MemoryKind) -> &'static str {
    kind.as_str()
}
