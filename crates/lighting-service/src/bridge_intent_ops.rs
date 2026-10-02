use lighting_core::{
    BridgeClaimResult, BridgeIntentRecord, BridgeIntentRepository, BridgeIntentRepositoryError,
};
use std::sync::Arc;
use thiserror::Error;

use crate::bridge_intent_dto::{
    ClaimBridgeIntentRequest, ClaimBridgeIntentResponse, CompleteBridgeIntentRequest,
};

#[derive(Debug, Error)]
pub enum BridgeIntentOperationError {
    #[error("Bridge intent store is unavailable")]
    Unavailable,
    #[error("Repository error: {0}")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Bridge intent not found: {0}")]
    NotFound(String),
}

impl From<BridgeIntentRepositoryError> for BridgeIntentOperationError {
    fn from(error: BridgeIntentRepositoryError) -> Self {
        match error {
            BridgeIntentRepositoryError::Unavailable => BridgeIntentOperationError::Unavailable,
            BridgeIntentRepositoryError::NotFound(id) => BridgeIntentOperationError::NotFound(id),
            BridgeIntentRepositoryError::Operation(e) => BridgeIntentOperationError::Repository(e),
        }
    }
}

#[derive(Clone)]
pub struct BridgeIntentService {
    repo: Arc<dyn BridgeIntentRepository>,
}

impl BridgeIntentService {
    pub fn new(repo: Arc<dyn BridgeIntentRepository>) -> Self {
        Self { repo }
    }

    pub async fn claim(
        &self,
        request: ClaimBridgeIntentRequest,
    ) -> Result<ClaimBridgeIntentResponse, BridgeIntentOperationError> {
        let result = self
            .repo
            .claim(
                &request.intent_id,
                &request.intent_commit,
                &request.canonical_digest,
                &request.action,
            )
            .await?;
        match result {
            BridgeClaimResult::Claimed { record } => {
                Ok(ClaimBridgeIntentResponse::Claimed { record })
            }
            BridgeClaimResult::Existing { record } => {
                Ok(ClaimBridgeIntentResponse::Existing { record })
            }
            BridgeClaimResult::Conflict {
                existing_digest,
                incoming_digest,
            } => Ok(ClaimBridgeIntentResponse::Conflict {
                existing_digest,
                incoming_digest,
            }),
        }
    }

    pub async fn complete(
        &self,
        request: CompleteBridgeIntentRequest,
    ) -> Result<BridgeIntentRecord, BridgeIntentOperationError> {
        self.repo
            .complete(
                &request.intent_id,
                &request.status,
                request.lantern_record_id,
                request.result_digest,
                request.error,
            )
            .await
            .map_err(Into::into)
    }

    pub async fn get(
        &self,
        intent_id: &str,
    ) -> Result<Option<BridgeIntentRecord>, BridgeIntentOperationError> {
        self.repo.get(intent_id).await.map_err(Into::into)
    }

    pub async fn list_all(&self) -> Result<Vec<BridgeIntentRecord>, BridgeIntentOperationError> {
        self.repo.list_all().await.map_err(Into::into)
    }
}
