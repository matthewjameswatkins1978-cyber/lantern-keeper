//! Application-layer Marker operations.
//!
//! Uses `MemoryPathRepository` for all Marker operations. Contains no
//! SurrealDB types, queries, record IDs, or normalisation logic.

use std::sync::Arc;

use lighting_core::{Marker, MarkerId, MemoryPathRepository, StoreMarkerResult};

use crate::marker_dto::MarkerResponse;
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum MarkerOperationError {
    #[error("memory-path repository is not available")]
    Unavailable,
    #[error("marker text must not be blank")]
    BlankText,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<MarkerOperationError> for ApiError {
    fn from(error: MarkerOperationError) -> Self {
        match error {
            MarkerOperationError::Unavailable => ApiError::storage_unavailable(),
            MarkerOperationError::BlankText => ApiError {
                code: "invalid_marker".to_owned(),
                message: "Marker text must not be blank".to_owned(),
            },
            MarkerOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct MarkerService {
    repo: Arc<dyn MemoryPathRepository>,
}

impl MarkerService {
    pub fn new(repo: Arc<dyn MemoryPathRepository>) -> Self {
        Self { repo }
    }

    /// Creates a new Marker. Returns the existing Marker if a normalised
    /// duplicate already exists (idempotent).
    pub async fn create_marker(
        &self,
        text: String,
    ) -> Result<(MarkerResponse, bool), MarkerOperationError> {
        let marker = Marker::new(text).map_err(|_| MarkerOperationError::BlankText)?;

        let result = self
            .repo
            .create_marker(marker)
            .await
            .map_err(|e| MarkerOperationError::Repository(e.into()))?;

        match result {
            StoreMarkerResult::Created(m) => Ok((MarkerResponse::from_domain(&m), true)),
            StoreMarkerResult::Existing(m) => Ok((MarkerResponse::from_domain(&m), false)),
        }
    }

    /// Retrieves a Marker by ID.
    pub async fn get_marker(
        &self,
        id: &MarkerId,
    ) -> Result<Option<MarkerResponse>, MarkerOperationError> {
        let marker = self
            .repo
            .get_marker(id)
            .await
            .map_err(|e| MarkerOperationError::Repository(e.into()))?;
        Ok(marker.as_ref().map(MarkerResponse::from_domain))
    }

    /// Finds a Marker by a remembered phrase.
    ///
    /// The phrase is normalised by the domain layer (via `Marker::new`),
    /// not by this service or HTTP layer.
    pub async fn lookup_marker(
        &self,
        phrase: &str,
    ) -> Result<Option<MarkerResponse>, MarkerOperationError> {
        // Normalise the phrase through Marker::new to get the lookup key,
        // then use the repository's find_marker_by_lookup.
        // Using Marker::new ensures we use the domain's normalisation.
        let normalised = Marker::new(phrase).map_err(|_| MarkerOperationError::BlankText)?;
        let lookup_key = normalised.lookup_key().to_owned();

        let marker = self
            .repo
            .find_marker_by_lookup(&lookup_key)
            .await
            .map_err(|e| MarkerOperationError::Repository(e.into()))?;
        Ok(marker.as_ref().map(MarkerResponse::from_domain))
    }
}
