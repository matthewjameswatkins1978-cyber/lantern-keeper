//! Candidate-only Dreamer output.
//!
//! Dreamer can interpret bounded evidence, but its output is never a canonical
//! mutation and this DTO intentionally has no authority-shaped fields.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    StateChange,
    Contradiction,
    Lesson,
    Association,
    Correction,
    NoChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub source_id: Option<String>,
    pub episode_id: Option<String>,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamerCandidate {
    pub kind: CandidateKind,
    pub subject: String,
    pub predicate: Option<String>,
    pub proposed_interpretation: String,
    pub supporting_source_ids: Vec<String>,
    pub evidence_references: Vec<EvidenceReference>,
    pub confidence: f32,
    pub reason: String,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub schema_version: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum DreamerCandidateError {
    #[error("candidate subject is blank")]
    BlankSubject,
    #[error("candidate interpretation is blank")]
    BlankInterpretation,
    #[error("candidate reason is blank")]
    BlankReason,
    #[error("candidate confidence must be finite and between 0 and 1")]
    InvalidConfidence,
    #[error("candidate must include evidence")]
    MissingEvidence,
    #[error("candidate provider/model/schema metadata is incomplete")]
    MissingMetadata,
}

impl DreamerCandidate {
    pub fn validate(&self) -> Result<(), DreamerCandidateError> {
        if self.subject.trim().is_empty() {
            return Err(DreamerCandidateError::BlankSubject);
        }
        if self.proposed_interpretation.trim().is_empty() {
            return Err(DreamerCandidateError::BlankInterpretation);
        }
        if self.reason.trim().is_empty() {
            return Err(DreamerCandidateError::BlankReason);
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(DreamerCandidateError::InvalidConfidence);
        }
        if !matches!(self.kind, CandidateKind::NoChange)
            && self.supporting_source_ids.is_empty()
            && self.evidence_references.is_empty()
        {
            return Err(DreamerCandidateError::MissingEvidence);
        }
        if self.provider.trim().is_empty()
            || self.model.trim().is_empty()
            || self.prompt_version.trim().is_empty()
            || self.schema_version.trim().is_empty()
        {
            return Err(DreamerCandidateError::MissingMetadata);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> DreamerCandidate {
        DreamerCandidate {
            kind: CandidateKind::StateChange,
            subject: "alex".to_owned(),
            predicate: Some("primary_editor".to_owned()),
            proposed_interpretation: "SuperEditor".to_owned(),
            supporting_source_ids: vec!["source-1".to_owned()],
            evidence_references: vec![],
            confidence: 0.91,
            reason: "newer direct-holder evidence".to_owned(),
            provider: "nebius".to_owned(),
            model: "configured-model".to_owned(),
            prompt_version: "dreamer-v1".to_owned(),
            schema_version: "candidate-v1".to_owned(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn candidate_is_validated_without_authority_fields() {
        assert!(candidate().validate().is_ok());
        let json = serde_json::to_value(candidate()).unwrap();
        assert!(json.get("authority_grant").is_none());
        assert!(json.get("authority_revocation").is_none());
    }

    #[test]
    fn authority_shaped_output_is_rejected_by_strict_dto() {
        let mut json = serde_json::to_value(candidate()).unwrap();
        json["authority_grant"] = serde_json::json!({"capability": "reboot"});
        assert!(serde_json::from_value::<DreamerCandidate>(json).is_err());
    }

    #[test]
    fn non_change_may_explain_an_empty_evidence_result() {
        let mut value = candidate();
        value.kind = CandidateKind::NoChange;
        value.supporting_source_ids.clear();
        assert!(value.validate().is_ok());
    }
}
