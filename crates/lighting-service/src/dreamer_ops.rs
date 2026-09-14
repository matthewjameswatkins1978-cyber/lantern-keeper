use std::sync::Arc;

use async_trait::async_trait;
use lighting_core::DreamerCandidate;
use reqwest::Client;

use crate::dreamer_dto::DreamerContext;

#[derive(Debug, thiserror::Error)]
pub enum DreamerOperationError {
    #[error("Nebius Dreamer is not configured")]
    NotConfigured,
    #[error("Dreamer request failed")]
    Request(#[source] reqwest::Error),
    #[error("Dreamer returned an invalid structured response: {0}")]
    InvalidResponse(String),
    #[error("Dreamer context is invalid: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait DreamerProvider: Send + Sync {
    async fn propose(
        &self,
        context: &DreamerContext,
    ) -> Result<DreamerCandidate, DreamerOperationError>;
}

#[derive(Clone)]
pub struct DreamerService {
    provider: Arc<dyn DreamerProvider>,
}

impl DreamerService {
    pub fn new(provider: Arc<dyn DreamerProvider>) -> Self {
        Self { provider }
    }

    pub async fn propose(
        &self,
        context: DreamerContext,
    ) -> Result<DreamerCandidate, DreamerOperationError> {
        context.validate()?;
        let candidate = self.provider.propose(&context).await?;
        candidate
            .validate()
            .map_err(|error| DreamerOperationError::InvalidResponse(error.to_string()))?;
        Ok(candidate)
    }
}

#[derive(Clone)]
pub struct NebiusDreamer {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl NebiusDreamer {
    pub fn from_env() -> Result<Self, DreamerOperationError> {
        let api_key = std::env::var("NEBIUS_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(DreamerOperationError::NotConfigured)?;
        let base_url = std::env::var("NEBIUS_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(DreamerOperationError::NotConfigured)?
            .trim_end_matches('/')
            .to_owned();
        let model = std::env::var("NEBIUS_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(DreamerOperationError::NotConfigured)?;
        Ok(Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
        })
    }

    fn system_prompt() -> &'static str {
        "You are Lantern Keeper Dreamer. Return exactly one JSON object matching candidate-v1. You may propose only state_change, contradiction, lesson, association, correction, or no_change. You are candidate-only: never return authority grants, revocations, approvals, permissions, or executable actions. Use only supplied evidence."
    }
}

#[async_trait]
impl DreamerProvider for NebiusDreamer {
    async fn propose(
        &self,
        context: &DreamerContext,
    ) -> Result<DreamerCandidate, DreamerOperationError> {
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "temperature": 0,
                "messages": [
                    {"role": "system", "content": Self::system_prompt()},
                    {"role": "user", "content": serde_json::to_string(context).map_err(|error| DreamerOperationError::InvalidResponse(error.to_string()))?}
                ],
                "response_format": {"type": "json_object"}
            }))
            .send()
            .await
            .map_err(DreamerOperationError::Request)?;
        if !response.status().is_success() {
            return Err(DreamerOperationError::InvalidResponse(format!(
                "provider returned HTTP {}",
                response.status()
            )));
        }
        let envelope: CompletionResponse = response
            .json()
            .await
            .map_err(DreamerOperationError::Request)?;
        let content = envelope
            .choices
            .first()
            .map(|choice| choice.message.content.as_str())
            .ok_or_else(|| {
                DreamerOperationError::InvalidResponse("no completion choice".to_owned())
            })?;
        let candidate: DreamerCandidate = serde_json::from_str(content)
            .map_err(|error| DreamerOperationError::InvalidResponse(error.to_string()))?;
        Ok(candidate)
    }
}

#[derive(serde::Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}

#[derive(serde::Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(serde::Deserialize)]
struct CompletionMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use lighting_core::CandidateKind;

    use crate::dreamer_dto::DreamerEvidence;

    struct FakeProvider {
        candidate: DreamerCandidate,
    }

    #[async_trait]
    impl DreamerProvider for FakeProvider {
        async fn propose(
            &self,
            _context: &DreamerContext,
        ) -> Result<DreamerCandidate, DreamerOperationError> {
            Ok(self.candidate.clone())
        }
    }

    fn context() -> DreamerContext {
        DreamerContext {
            task: "find state changes".to_owned(),
            evidence: vec![DreamerEvidence {
                source_id: "source-1".to_owned(),
                episode_id: Some("episode-1".to_owned()),
                evidence_text: "I now use SuperEditor everywhere".to_owned(),
            }],
        }
    }

    fn candidate() -> DreamerCandidate {
        DreamerCandidate {
            kind: CandidateKind::StateChange,
            subject: "alex".to_owned(),
            predicate: Some("primary_editor".to_owned()),
            proposed_interpretation: "SuperEditor".to_owned(),
            supporting_source_ids: vec!["source-1".to_owned()],
            evidence_references: vec![],
            confidence: 0.9,
            reason: "newer evidence".to_owned(),
            provider: "test".to_owned(),
            model: "fixture".to_owned(),
            prompt_version: "dreamer-v1".to_owned(),
            schema_version: "candidate-v1".to_owned(),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn service_returns_candidate_without_canonical_mutation() {
        let service = DreamerService::new(Arc::new(FakeProvider {
            candidate: candidate(),
        }));
        let result = service.propose(context()).await.unwrap();
        assert_eq!(result.kind, CandidateKind::StateChange);
    }

    #[tokio::test]
    async fn invalid_candidate_fails_closed() {
        let mut invalid = candidate();
        invalid.confidence = f32::NAN;
        let service = DreamerService::new(Arc::new(FakeProvider { candidate: invalid }));
        assert!(matches!(
            service.propose(context()).await,
            Err(DreamerOperationError::InvalidResponse(_))
        ));
    }
}
