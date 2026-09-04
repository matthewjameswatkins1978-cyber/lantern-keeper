use std::sync::Arc;

use lighting_core::{
    LedgerEvent, LedgerEventRepository, LedgerIngestResult, LedgerRepositoryError,
};

use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum LedgerOperationError {
    #[error("ledger repository is not available")]
    Unavailable,
    #[error("invalid ledger event: {0}")]
    Invalid(String),
    #[error("ledger repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<LedgerOperationError> for ApiError {
    fn from(error: LedgerOperationError) -> Self {
        match error {
            LedgerOperationError::Unavailable => ApiError::storage_unavailable(),
            LedgerOperationError::Invalid(message) => ApiError {
                code: "invalid_ledger_event".to_owned(),
                message,
            },
            LedgerOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct LedgerService {
    repo: Arc<dyn LedgerEventRepository>,
}

impl LedgerService {
    pub fn new(repo: Arc<dyn LedgerEventRepository>) -> Self {
        Self { repo }
    }

    pub async fn ingest(
        &self,
        event: LedgerEvent,
    ) -> Result<crate::ledger_dto::LedgerEventResponse, LedgerOperationError> {
        match self.repo.ingest(event).await.map_err(map_error)? {
            LedgerIngestResult::Stored(event) => Ok(crate::ledger_dto::LedgerEventResponse {
                event,
                duplicate: false,
            }),
            LedgerIngestResult::Duplicate(event) => Ok(crate::ledger_dto::LedgerEventResponse {
                event,
                duplicate: true,
            }),
        }
    }
}

fn map_error(error: LedgerRepositoryError) -> LedgerOperationError {
    match error {
        LedgerRepositoryError::Operation(error) => LedgerOperationError::Repository(error),
    }
}
