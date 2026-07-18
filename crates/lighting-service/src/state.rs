use std::sync::Arc;

use crate::project_ops::ProjectService;
use crate::source_ops::SourceService;

#[derive(Clone)]
pub struct AppState {
    pub ready: Arc<std::sync::atomic::AtomicBool>,
    pub source_service: Option<SourceService>,
    pub project_service: Option<ProjectService>,
}

impl AppState {
    pub fn new_unready() -> Self {
        Self {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            source_service: None,
            project_service: None,
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
