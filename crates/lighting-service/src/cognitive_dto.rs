use lighting_core::DreamerCandidate;
use serde::{Deserialize, Serialize};

use crate::{dreamer_ops::DreamerCallMetadata, tavily::ExternalEvidenceRecord};

#[derive(Debug, Deserialize)]
pub struct CognitiveRequest {
    pub query: String,
    #[serde(default = "default_task")]
    pub task: String,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_task() -> String {
    "Interpret the external evidence as a candidate understanding change. Preserve uncertainty and never infer authority.".to_owned()
}

fn default_max_results() -> usize {
    3
}

#[derive(Debug, Serialize)]
pub struct CognitiveResponse {
    pub evidence: Vec<ExternalEvidenceRecord>,
    pub candidate: DreamerCandidate,
    pub dreamer: Option<DreamerCallMetadata>,
    pub canonical_mutation: bool,
    pub authority_changed: bool,
}
