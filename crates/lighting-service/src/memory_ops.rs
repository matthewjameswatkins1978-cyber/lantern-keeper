use std::sync::Arc;

use chrono::Utc;
use lighting_core::{
    Memory, MemoryId, MemoryKind, MemoryRepository, MemoryRepositoryError, MemorySearchQuery,
    NewMemory, ProjectId,
};

use crate::memory_dto::{
    ContextResponse, RecallResponse, RememberRequest, RetrievalTrace, SupersedeResponse,
};
use crate::source_dto::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum MemoryOperationError {
    #[error("memory repository is not available")]
    Unavailable,
    #[error("invalid memory request: {0}")]
    Invalid(String),
    #[error("memory not found")]
    NotFound,
    #[error("memory repository operation failed")]
    Repository(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<MemoryOperationError> for ApiError {
    fn from(error: MemoryOperationError) -> Self {
        match error {
            MemoryOperationError::Unavailable => ApiError::storage_unavailable(),
            MemoryOperationError::Invalid(message) => ApiError {
                code: "invalid_memory".to_owned(),
                message,
            },
            MemoryOperationError::NotFound => ApiError {
                code: "memory_not_found".to_owned(),
                message: "No Memory exists with the given ID".to_owned(),
            },
            MemoryOperationError::Repository(_) => ApiError::internal_error(),
        }
    }
}

#[derive(Clone)]
pub struct MemoryService {
    repo: Arc<dyn MemoryRepository>,
}

impl MemoryService {
    pub fn new(repo: Arc<dyn MemoryRepository>) -> Self {
        Self { repo }
    }

    pub async fn remember(&self, request: RememberRequest) -> Result<Memory, MemoryOperationError> {
        let kind = MemoryKind::parse(&request.kind)
            .ok_or_else(|| MemoryOperationError::Invalid("kind is not supported".to_owned()))?;
        let project_id = request
            .project_id
            .map(ProjectId::new)
            .transpose()
            .map_err(|_| MemoryOperationError::Invalid("project_id cannot be blank".to_owned()))?;
        let now = Utc::now();
        let memory = Memory::new(NewMemory {
            content: request.content,
            kind,
            project_id,
            confidence: request.confidence,
            importance: request.importance,
            recorded_at: now,
            known_at: now,
            observed_at: request.observed_at,
            valid_from: now,
            valid_until: None,
            derived_from: request.derived_from,
            updates: request.updates,
            extends: request.extends,
            supersedes: request.supersedes,
            contradicts: request.contradicts,
            supports: request.supports,
            agent: request.agent,
        })
        .map_err(|error| MemoryOperationError::Invalid(error.to_string()))?;

        self.repo.store(memory).await.map_err(map_repository_error)
    }

    pub async fn recall(
        &self,
        project_id: Option<String>,
        phrase: Option<String>,
        include_inactive: bool,
        as_of: Option<chrono::DateTime<Utc>>,
    ) -> Result<RecallResponse, MemoryOperationError> {
        let project_id = project_id
            .map(ProjectId::new)
            .transpose()
            .map_err(|_| MemoryOperationError::Invalid("project_id cannot be blank".to_owned()))?;
        let phrase = phrase
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let query = MemorySearchQuery {
            project_id,
            phrase: phrase.clone(),
            include_inactive,
            as_of,
        };
        let memories = self
            .repo
            .search(&query)
            .await
            .map_err(map_repository_error)?;
        let ids: Vec<String> = memories
            .iter()
            .map(|memory| memory.id.to_string())
            .collect();
        let abstained = memories.is_empty();
        Ok(RecallResponse {
            memories,
            abstained,
            reason: abstained.then(|| "Lantern has no reliable matching active Memory.".to_owned()),
            trace: RetrievalTrace {
                query: phrase,
                candidate_memory_ids: ids.clone(),
                selected_memory_ids: ids,
                channel: "lexical-project-status".to_owned(),
            },
        })
    }

    pub async fn context(
        &self,
        project_id: Option<String>,
        query: Option<String>,
    ) -> Result<ContextResponse, MemoryOperationError> {
        let recall = self.recall(project_id, query, false, None).await?;
        let mut sections = [
            ("CURRENT STATE", Vec::new()),
            ("IMPORTANT DECISIONS", Vec::new()),
            ("RELEVANT FACTS", Vec::new()),
            ("LESSONS / GOTCHAS", Vec::new()),
            ("OPEN LOOPS", Vec::new()),
            ("OTHER ACTIVE MEMORY", Vec::new()),
        ];
        for memory in &recall.memories {
            let section = match memory.kind {
                MemoryKind::Summary | MemoryKind::Workflow => 0,
                MemoryKind::Decision => 1,
                MemoryKind::Fact | MemoryKind::Preference | MemoryKind::Instruction => 2,
                MemoryKind::Lesson | MemoryKind::Gotcha => 3,
                MemoryKind::OpenLoop => 4,
                MemoryKind::Entity => 5,
            };
            sections[section].1.push(memory);
        }
        let mut context = String::from("# Lantern Working Context\n\n");
        for (title, memories) in sections {
            context.push_str("## ");
            context.push_str(title);
            context.push('\n');
            if memories.is_empty() {
                context.push_str("None recorded.\n\n");
            } else {
                for memory in memories {
                    context.push_str(&format!(
                        "- [{} | confidence {:.2}] {}\n",
                        memory.kind.as_str(),
                        memory.confidence,
                        memory.content
                    ));
                }
                context.push('\n');
            }
        }
        Ok(ContextResponse {
            context,
            memories: recall.memories,
            abstained: recall.abstained,
            reason: recall.reason,
            trace: recall.trace,
        })
    }

    pub async fn supersede(&self, id: &str) -> Result<SupersedeResponse, MemoryOperationError> {
        let id = MemoryId::new(id.to_owned())
            .map_err(|_| MemoryOperationError::Invalid("memory_id cannot be blank".to_owned()))?;
        let memory = self
            .repo
            .supersede(&id, Utc::now())
            .await
            .map_err(map_repository_error)?;
        Ok(SupersedeResponse {
            status: memory.status,
            superseded_at: memory.superseded_at,
            memory,
        })
    }
}

fn map_repository_error(error: MemoryRepositoryError) -> MemoryOperationError {
    match error {
        MemoryRepositoryError::Unavailable => MemoryOperationError::Unavailable,
        MemoryRepositoryError::NotFound => MemoryOperationError::NotFound,
        MemoryRepositoryError::Operation(error) => MemoryOperationError::Repository(error),
    }
}
