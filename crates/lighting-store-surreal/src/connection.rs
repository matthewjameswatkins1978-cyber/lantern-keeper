use surrealdb::{
    engine::remote::ws::{Client, Ws},
    opt::auth::Root,
    Surreal,
};
use thiserror::Error;

use crate::{config::StoreConfig, schema};

#[derive(Clone)]
pub struct SurrealStore {
    db: Surreal<Client>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to connect to SurrealDB at {endpoint}: {source}")]
    Connect {
        endpoint: String,
        #[source]
        source: surrealdb::Error,
    },
    #[error("failed to authenticate with SurrealDB: {0}")]
    Authenticate(#[source] surrealdb::Error),
    #[error("failed to select SurrealDB namespace/database: {0}")]
    Select(#[source] surrealdb::Error),
    #[error("SurrealDB health check failed: {0}")]
    Health(#[source] surrealdb::Error),
    #[error("SurrealDB schema initialisation failed: {0}")]
    Schema(#[source] surrealdb::Error),
}

impl SurrealStore {
    pub async fn connect(config: &StoreConfig) -> Result<Self, StoreError> {
        let db = Surreal::new::<Ws>(config.endpoint.as_str())
            .await
            .map_err(|source| StoreError::Connect {
                endpoint: config.endpoint.clone(),
                source,
            })?;

        if !config.username.is_empty() || !config.password.is_empty() {
            db.signin(Root {
                username: config.username.clone(),
                password: config.password.clone(),
            })
            .await
            .map_err(StoreError::Authenticate)?;
        }

        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(StoreError::Select)?;

        Ok(Self { db })
    }

    pub async fn health_check(&self) -> Result<(), StoreError> {
        self.db
            .query("RETURN true;")
            .await
            .map(|_| ())
            .map_err(StoreError::Health)
    }

    pub async fn initialise_schema(&self) -> Result<(), StoreError> {
        self.db
            .query(schema::BOOTSTRAP_QUERY)
            .await
            .map(|_| ())
            .map_err(StoreError::Schema)
    }
}
