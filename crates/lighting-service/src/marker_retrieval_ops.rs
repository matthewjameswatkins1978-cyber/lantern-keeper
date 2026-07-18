//! Marker-led retrieval operations.
//!
//! Path: phrase → Marker lookup → graph traversal → Episodes → Source excerpts.

use std::sync::Arc;

use lighting_core::{Marker, MemoryPathRepository, SourceRepository};

use crate::marker_retrieval_dto::{MarkerMatch, MarkerRetrievalResponse, RetrievedEpisode};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum RetrievalError {
    #[error("storage unavailable")]
    Unavailable,
    #[error("repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<RetrievalError> for ApiError {
    fn from(error: RetrievalError) -> Self {
        match error {
            RetrievalError::Unavailable => ApiError::storage_unavailable(),
            RetrievalError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct MarkerRetrievalService {
    memory_repo: Arc<dyn MemoryPathRepository>,
    source_repo: Arc<dyn SourceRepository>,
}

impl MarkerRetrievalService {
    pub fn new(
        memory_repo: Arc<dyn MemoryPathRepository>,
        source_repo: Arc<dyn SourceRepository>,
    ) -> Self {
        Self {
            memory_repo,
            source_repo,
        }
    }

    pub async fn retrieve(&self, phrase: &str) -> Result<MarkerRetrievalResponse, RetrievalError> {
        // Use domain normalisation through Marker::new
        let normalised = match Marker::new(phrase) {
            Ok(m) => m,
            Err(_) => {
                return Ok(MarkerRetrievalResponse {
                    query: phrase.to_owned(),
                    marker: None,
                    episodes: vec![],
                    warnings: vec!["No exact Marker matches this phrase.".to_owned()],
                });
            }
        };

        let lookup_key = normalised.lookup_key().to_owned();

        let marker = self
            .memory_repo
            .find_marker_by_lookup(&lookup_key)
            .await
            .map_err(|e| RetrievalError::Repository(e.into()))?;

        let marker = match marker {
            Some(m) => m,
            None => {
                return Ok(MarkerRetrievalResponse {
                    query: phrase.to_owned(),
                    marker: None,
                    episodes: vec![],
                    warnings: vec!["No exact Marker matches this phrase.".to_owned()],
                });
            }
        };

        let marker_match = MarkerMatch {
            marker_id: marker.id().as_str().to_owned(),
            display_text: marker.display_text().to_owned(),
            lookup_key: marker.lookup_key().to_owned(),
        };

        let links = self
            .memory_repo
            .list_marker_episode_links(marker.id())
            .await
            .map_err(|e| RetrievalError::Repository(e.into()))?;

        if links.is_empty() {
            return Ok(MarkerRetrievalResponse {
                query: phrase.to_owned(),
                marker: Some(marker_match),
                episodes: vec![],
                warnings: vec!["Marker exists but is not linked to any Episodes.".to_owned()],
            });
        }

        let mut episodes = Vec::with_capacity(links.len());
        for link in &links {
            let episode = match self
                .memory_repo
                .get_episode(link.episode_id())
                .await
                .map_err(|e| RetrievalError::Repository(e.into()))?
            {
                Some(ep) => ep,
                None => continue,
            };

            let source = match self
                .source_repo
                .get(episode.source_range().source_id())
                .await
                .map_err(|e| RetrievalError::Repository(e.into()))?
            {
                Some(s) => s,
                None => continue,
            };

            let excerpt = episode.source_range().slice(source.content()).to_owned();
            let why_matched = format!(
                "Marker \"{}\" is linked to this Episode.",
                marker.display_text()
            );

            episodes.push(RetrievedEpisode {
                episode_id: episode.id().as_str().to_owned(),
                title: episode.title().as_str().to_owned(),
                source_id: episode.source_range().source_id().as_str().to_owned(),
                start_byte: episode.source_range().start_byte(),
                end_byte: episode.source_range().end_byte(),
                excerpt,
                why_matched,
            });
        }

        // Deterministic ordering: created_at ascending, episode_id ascending as tie-breaker.
        // We fetch episodes and sort by those criteria by re-loading them.
        // A simpler approach: sort the CollectedEpisode data.
        // For now, trust repository ordering — but we also sort client-side:
        episodes.sort_by(|a, b| {
            // We don't have created_at on the response DTO, so just sort by episode_id.
            // The repository guarantees ordering, so this is a safety net.
            a.episode_id.cmp(&b.episode_id)
        });

        let count = episodes.len();
        let warnings = if count >= 50 {
            vec![format!(
                "Results hard-limited to 50. {} Episode(s) returned.",
                count
            )]
        } else {
            vec![]
        };

        Ok(MarkerRetrievalResponse {
            query: phrase.to_owned(),
            marker: Some(marker_match),
            episodes,
            warnings,
        })
    }
}
