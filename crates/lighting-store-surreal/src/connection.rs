use std::time::Duration;

use surrealdb::{
    engine::remote::ws::{Client, Ws},
    opt::auth::Root,
    Surreal,
};
use thiserror::Error;
use tokio::time::timeout;

use crate::{
    config::{ConfigError, StoreConfig},
    schema,
};

#[derive(Clone)]
pub struct SurrealStore {
    db: Surreal<Client>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("invalid SurrealDB configuration: {0}")]
    Config(#[from] ConfigError),
    #[error("failed to connect to SurrealDB at {endpoint}: {source}")]
    Connect {
        endpoint: String,
        #[source]
        source: Box<surrealdb::Error>,
    },
    #[error("failed to authenticate with SurrealDB: {0}")]
    Authenticate(#[source] Box<surrealdb::Error>),
    #[error("failed to select SurrealDB namespace/database: {0}")]
    Select(#[source] Box<surrealdb::Error>),
    #[error("SurrealDB health check failed: {0}")]
    Health(#[source] Box<surrealdb::Error>),
    #[error("SurrealDB health check timed out")]
    HealthTimeout,
    #[error("SurrealDB schema initialisation failed: {0}")]
    Schema(#[source] Box<surrealdb::Error>),
}

impl SurrealStore {
    pub async fn connect(config: &StoreConfig) -> Result<Self, StoreError> {
        config.validate()?;

        let db = Surreal::new::<Ws>(config.websocket_address())
            .await
            .map_err(|source| StoreError::Connect {
                endpoint: config.endpoint.clone(),
                source: Box::new(source),
            })?;

        if !config.username.is_empty() || !config.password.is_empty() {
            db.signin(Root {
                username: config.username.clone(),
                password: config.password.clone(),
            })
            .await
            .map_err(|source| StoreError::Authenticate(Box::new(source)))?;
        }

        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(|source| StoreError::Select(Box::new(source)))?;

        Ok(Self { db })
    }

    pub async fn health_check(&self) -> Result<(), StoreError> {
        timeout(Duration::from_secs(2), self.db.query("RETURN true;"))
            .await
            .map_err(|_| StoreError::HealthTimeout)?
            .map(|_| ())
            .map_err(|source| StoreError::Health(Box::new(source)))
    }

    pub async fn initialise_schema(&self) -> Result<(), StoreError> {
        self.db
            .query(schema::BOOTSTRAP_QUERY)
            .await
            .map(|_| ())
            .map_err(|source| StoreError::Schema(Box::new(source)))
    }

    /// Executes a raw SurrealQL query against the underlying connection.
    pub fn query<'a>(
        &'a self,
        query: impl Into<std::borrow::Cow<'a, str>>,
    ) -> surrealdb::method::Query<'a, Client> {
        self.db.query(query)
    }
}
