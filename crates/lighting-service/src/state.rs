use std::sync::Arc;

use crate::authority_ops::AuthorityService;
use crate::episode_association_ops::EpisodeAssociationService;
use crate::episode_ops::EpisodeService;
use crate::epistemic_ops::EpistemicService;
use crate::ledger_ops::LedgerService;
use crate::marker_ops::MarkerService;
use crate::marker_retrieval_ops::MarkerRetrievalService;
use crate::memory_ops::MemoryService;
use crate::project_ops::ProjectService;
use crate::project_retrieval_ops::ProjectRetrievalService;
use crate::source_ops::SourceService;
use crate::tethers_engine_client::TethersEngineClient;

#[derive(Clone)]
pub struct AppState {
    pub ready: Arc<std::sync::atomic::AtomicBool>,
    pub source_service: Option<SourceService>,
    pub project_service: Option<ProjectService>,
    pub marker_service: Option<MarkerService>,
    pub episode_service: Option<EpisodeService>,
    pub association_service: Option<EpisodeAssociationService>,
    pub retrieval_service: Option<MarkerRetrievalService>,
    pub project_retrieval_service: Option<ProjectRetrievalService>,
    pub tethers_client: Option<TethersEngineClient>,
    pub memory_service: Option<MemoryService>,
    pub ledger_service: Option<LedgerService>,
    pub epistemic_service: Option<EpistemicService>,
    pub authority_service: Option<AuthorityService>,
}

impl AppState {
    pub fn new_unready() -> Self {
        Self {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: None,
            project_service: None,
            marker_service: None,
            episode_service: None,
            association_service: None,
            retrieval_service: None,
            project_retrieval_service: None,
            tethers_client: None,
            memory_service: None,
            ledger_service: None,
            epistemic_service: None,
            authority_service: None,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new_unready()
    }
}
