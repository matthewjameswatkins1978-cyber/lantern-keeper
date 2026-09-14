use lighting_core::DreamerCandidate;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DreamerEvidence {
    pub source_id: String,
    pub episode_id: Option<String>,
    pub evidence_text: String,
    /// External evidence is retained as evidence but must remain visibly
    /// external throughout the Dreamer request and candidate explanation.
    pub external: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DreamerContext {
    pub task: String,
    pub evidence: Vec<DreamerEvidence>,
}

impl DreamerContext {
    pub fn validate(&self) -> Result<(), DreamerOperationError> {
        if self.task.trim().is_empty() {
            return Err(DreamerOperationError::Invalid(
                "task must not be blank".to_owned(),
            ));
        }
        if self.evidence.len() > 32 {
            return Err(DreamerOperationError::Invalid(
                "evidence is limited to 32 records".to_owned(),
            ));
        }
        if self.evidence.iter().any(|evidence| {
            evidence.source_id.trim().is_empty() || evidence.evidence_text.len() > 16_384
        }) {
            return Err(DreamerOperationError::Invalid(
                "evidence ids must be present and evidence is limited to 16 KiB".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DreamerRequest {
    pub context: DreamerContext,
}

#[derive(Clone, Debug, Serialize)]
pub struct DreamerResponse {
    pub candidate: DreamerCandidate,
    pub canonical_mutation: bool,
}

use crate::dreamer_ops::DreamerOperationError;
