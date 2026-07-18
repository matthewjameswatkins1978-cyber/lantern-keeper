use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use lighting_store_surreal::{StoreConfig, SurrealStore};
use tokio::time::timeout;

pub type HealthCheckFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub trait DatabaseHealth: Send + Sync {
    fn health_check(&self) -> HealthCheckFuture<'_>;
}

#[derive(Clone)]
pub struct AppState {
    database: Arc<dyn DatabaseHealth>,
}

impl AppState {
    pub fn new(database: impl DatabaseHealth + 'static) -> Self {
        Self {
            database: Arc::new(database),
        }
    }

    pub async fn database_health(&self) -> Result<(), String> {
        self.database.health_check().await
    }
}

impl DatabaseHealth for SurrealStore {
    fn health_check(&self) -> HealthCheckFuture<'_> {
        Box::pin(async move { self.health_check().await.map_err(|error| error.to_string()) })
    }
}

impl DatabaseHealth for StoreConfig {
    fn health_check(&self) -> HealthCheckFuture<'_> {
        Box::pin(async move {
            timeout(Duration::from_secs(3), async {
                let store = SurrealStore::connect(self).await?;
                store.health_check().await
            })
            .await
            .map_err(|_| "SurrealDB health check timed out".to_owned())?
            .map_err(|error| error.to_string())
        })
    }
}
