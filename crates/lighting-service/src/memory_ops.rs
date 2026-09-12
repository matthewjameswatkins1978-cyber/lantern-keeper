use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use lighting_core::{
    MemoryId, MemoryRelation, MemoryRelationRepository, MemoryRepository, MemoryRepositoryError,
    MemoryState,
};

use crate::memory_dto::{MemoryContextResponse, MemorySearchHit, RememberResponse};

#[derive(Debug, thiserror::Error)]
pub enum MemoryOperationError {
    #[error("memory repository is not available")]
    Unavailable,
    #[error("invalid memory request: {0}")]
    Invalid(&'static str),
    #[error("memory repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<MemoryOperationError> for crate::source_dto::ApiError {
    fn from(error: MemoryOperationError) -> Self {
        match error {
            MemoryOperationError::Invalid(message) => Self {
                code: "invalid_memory".to_owned(),
                message: message.to_owned(),
            },
            MemoryOperationError::Unavailable => Self::storage_unavailable(),
            MemoryOperationError::Repository(_) => Self::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct MemoryService {
    repo: Arc<dyn MemoryRepository>,
    relation_repo: Option<Arc<dyn MemoryRelationRepository>>,
}

impl MemoryService {
    pub fn new(repo: Arc<dyn MemoryRepository>) -> Self {
        Self {
            repo,
            relation_repo: None,
        }
    }

    pub fn with_relations(
        repo: Arc<dyn MemoryRepository>,
        relation_repo: Arc<dyn MemoryRelationRepository>,
    ) -> Self {
        Self {
            repo,
            relation_repo: Some(relation_repo),
        }
    }

    pub async fn remember(
        &self,
        request: crate::memory_dto::RememberRequest,
    ) -> Result<RememberResponse, MemoryOperationError> {
        let candidate = request
            .into_candidate()
            .map_err(MemoryOperationError::Invalid)?;
        let decision = self
            .repo
            .reconcile_and_apply(candidate)
            .await
            .map_err(repo_error)?;
        Ok(RememberResponse {
            action: decision.action,
            reason: decision.reason,
            affected_memory_ids: decision.matched_memory_ids,
        })
    }

    pub async fn search(
        &self,
        query: &str,
        scope: Option<&str>,
        include_history: bool,
        limit: usize,
    ) -> Result<Vec<MemorySearchHit>, MemoryOperationError> {
        if query.trim().is_empty() {
            return Err(MemoryOperationError::Invalid(
                "search query must not be blank",
            ));
        }
        let words = tokens(query);
        let mut hits: Vec<_> = self
            .repo
            .list(scope, include_history)
            .await
            .map_err(repo_error)?
            .into_iter()
            .filter_map(|memory| {
                let haystack =
                    format!("{} {}", memory.canonical_text, memory.identity_key).to_lowercase();
                let score = words
                    .iter()
                    .filter(|word| haystack.contains(word.as_str()))
                    .count() as u32;
                (score > 0).then(|| MemorySearchHit {
                    memory,
                    score,
                    why: "lexical match against canonical text or identity".to_owned(),
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.memory.importance.cmp(&a.memory.importance))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    pub async fn context(
        &self,
        request: crate::memory_dto::MemoryContextRequest,
    ) -> Result<MemoryContextResponse, MemoryOperationError> {
        let all = self
            .search(
                &request.query,
                request.scope.as_deref(),
                request.include_history.unwrap_or(false),
                request.budget.unwrap_or(24),
            )
            .await?;
        let mut current_truth = Vec::new();
        let mut history = Vec::new();
        let mut unresolved = Vec::new();
        for hit in all {
            match hit.memory.state {
                MemoryState::Active => current_truth.push(hit),
                MemoryState::NeedsReview => unresolved.push(hit),
                _ => history.push(hit),
            }
        }
        Ok(MemoryContextResponse {
            current_truth,
            history,
            unresolved,
        })
    }

    pub async fn get(
        &self,
        id: &str,
    ) -> Result<Option<lighting_core::Memory>, MemoryOperationError> {
        let id = MemoryId::from_string(id)
            .map_err(|_| MemoryOperationError::Invalid("memory ID must not be blank"))?;
        self.repo.get(&id).await.map_err(repo_error)
    }

    pub async fn history(
        &self,
        id: &str,
    ) -> Result<Vec<lighting_core::Memory>, MemoryOperationError> {
        let memory = self
            .get(id)
            .await?
            .ok_or(MemoryOperationError::Invalid("memory was not found"))?;
        let memories = self
            .repo
            .list(memory.scope.as_deref(), true)
            .await
            .map_err(repo_error)?;
        Ok(memories
            .into_iter()
            .filter(|candidate| candidate.identity_key == memory.identity_key)
            .collect())
    }

    pub async fn doctor(&self) -> Result<serde_json::Value, MemoryOperationError> {
        let memories = self.repo.list(None, true).await.map_err(repo_error)?;
        let active = memories
            .iter()
            .filter(|memory| memory.state == MemoryState::Active)
            .count();
        let needs_review = memories
            .iter()
            .filter(|memory| memory.state == MemoryState::NeedsReview)
            .count();
        let checksum_failures = memories
            .iter()
            .filter(|memory| memory.checksum != memory.compute_checksum())
            .count();
        let relation_count = match &self.relation_repo {
            Some(repo) => repo.list_relations(None).await.map_err(repo_error)?.len(),
            None => 0,
        };
        Ok(serde_json::json!({
            "healthy": checksum_failures == 0,
            "memory_count": memories.len(),
            "active_count": active,
            "needs_review_count": needs_review,
            "checksum_failures": checksum_failures,
            "relation_count": relation_count,
        }))
    }

    pub async fn audit(&self) -> Result<serde_json::Value, MemoryOperationError> {
        let memories = self.repo.list(None, true).await.map_err(repo_error)?;
        let ids: HashSet<_> = memories.iter().map(|memory| memory.id.clone()).collect();
        let mut active_by_identity: HashMap<
            (Option<String>, String, lighting_core::MemoryKind),
            usize,
        > = HashMap::new();
        let mut duplicate_active = 0usize;
        let mut dangling_derivations = 0usize;
        let mut stale_derivations = 0usize;
        let mut invalid_supersession_links = 0usize;
        for memory in &memories {
            if memory.state == MemoryState::Active {
                let key = (
                    memory.scope.clone(),
                    memory.identity_key.clone(),
                    memory.kind,
                );
                let count = active_by_identity.entry(key).or_default();
                *count += 1;
                if *count > 1 {
                    duplicate_active += 1;
                }
            }
            for dependency in &memory.derived_from {
                match memories
                    .iter()
                    .find(|candidate| candidate.id == *dependency)
                {
                    None => dangling_derivations += 1,
                    Some(parent) if parent.state != MemoryState::Active => stale_derivations += 1,
                    Some(_) => {}
                }
            }
            if let Some(next) = &memory.superseded_by {
                if !ids.contains(next) || next == &memory.id {
                    invalid_supersession_links += 1;
                }
            }
        }
        let relation_count = match &self.relation_repo {
            Some(repo) => repo.list_relations(None).await.map_err(repo_error)?.len(),
            None => 0,
        };
        Ok(serde_json::json!({
            "healthy": duplicate_active == 0 && dangling_derivations == 0 && invalid_supersession_links == 0,
            "memory_count": memories.len(),
            "duplicate_active_identities": duplicate_active,
            "dangling_derivations": dangling_derivations,
            "stale_derivations": stale_derivations,
            "invalid_supersession_links": invalid_supersession_links,
            "relation_count": relation_count,
        }))
    }

    pub async fn export(&self) -> Result<Vec<lighting_core::Memory>, MemoryOperationError> {
        self.repo.list(None, true).await.map_err(repo_error)
    }

    pub async fn forget(
        &self,
        id: &str,
    ) -> Result<Option<lighting_core::Memory>, MemoryOperationError> {
        let id = MemoryId::from_string(id)
            .map_err(|_| MemoryOperationError::Invalid("memory ID must not be blank"))?;
        self.repo.forget(&id).await.map_err(repo_error)
    }

    pub async fn remember_relation(
        &self,
        request: crate::memory_dto::MemoryRelationRequest,
    ) -> Result<MemoryRelation, MemoryOperationError> {
        let Some(repo) = &self.relation_repo else {
            return Err(MemoryOperationError::Unavailable);
        };
        let relation = request
            .into_domain()
            .map_err(MemoryOperationError::Invalid)?;
        repo.upsert_relation(relation).await.map_err(repo_error)
    }

    pub async fn relations(
        &self,
        id: Option<&str>,
    ) -> Result<Vec<MemoryRelation>, MemoryOperationError> {
        let Some(repo) = &self.relation_repo else {
            return Err(MemoryOperationError::Unavailable);
        };
        let memory_id = id
            .map(MemoryId::from_string)
            .transpose()
            .map_err(|_| MemoryOperationError::Invalid("memory ID must not be blank"))?;
        repo.list_relations(memory_id.as_ref())
            .await
            .map_err(repo_error)
    }
}

fn repo_error(error: MemoryRepositoryError) -> MemoryOperationError {
    MemoryOperationError::Repository(Box::new(error))
}

fn tokens(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(str::to_lowercase)
        .collect()
}
