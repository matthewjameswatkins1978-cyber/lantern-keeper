use std::time::Duration;

use surrealdb::{
    Surreal,
    engine::any::{self, Any},
    opt::{Config, auth::Root},
};
use thiserror::Error;
use tokio::time::timeout;

use crate::{
    config::{ConfigError, StoreConfig},
    schema,
};

#[derive(Clone)]
pub struct SurrealStore {
    db: Surreal<Any>,
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
    #[error("failed to read the connected SurrealDB server version: {0}")]
    ServerVersion(#[source] Box<surrealdb::Error>),
    #[error("failed to read the Lantern schema version: {0}")]
    SchemaVersion(#[source] Box<surrealdb::Error>),
    #[error("SurrealDB schema initialisation failed: {0}")]
    Schema(#[source] Box<surrealdb::Error>),
}

impl SurrealStore {
    pub async fn connect(config: &StoreConfig) -> Result<Self, StoreError> {
        config.validate()?;

        let root = Root {
            username: config.username.clone(),
            password: config.password.clone(),
        };
        let connection_config = if config.username.is_empty() && config.password.is_empty() {
            Config::new()
        } else {
            Config::new().user(root.clone())
        };
        let endpoint = if config.uses_embedded_surrealkv() {
            format!(
                "surrealkv://{}?versioned=true&sync=every",
                config.path.display()
            )
        } else {
            config.endpoint.clone()
        };

        let db = any::connect((endpoint.clone(), connection_config))
            .await
            .map_err(|source| StoreError::Connect {
                endpoint,
                source: Box::new(source),
            })?;

        if !config.uses_embedded_surrealkv()
            && (!config.username.is_empty() || !config.password.is_empty())
        {
            db.signin(root)
                .await
                .map_err(|error| StoreError::Authenticate(Box::new(error)))?;
        }

        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(|error| StoreError::Select(Box::new(error)))?;

        Ok(Self { db })
    }

    pub async fn health_check(&self) -> Result<(), StoreError> {
        timeout(Duration::from_secs(2), self.db.query("RETURN true;"))
            .await
            .map_err(|_| StoreError::HealthTimeout)?
            .map(|_| ())
            .map_err(|error| StoreError::Health(Box::new(error)))
    }

    pub async fn server_version(&self) -> Result<Option<String>, StoreError> {
        let version = self
            .db
            .version()
            .await
            .map_err(|error| StoreError::ServerVersion(Box::new(error)))?;
        Ok(Some(version.to_string()))
    }

    pub async fn schema_version(&self) -> Result<Option<i64>, StoreError> {
        let mut response = self
            .db
            .query("SELECT VALUE schema_version FROM __lighting_schema:bootstrap;")
            .await
            .map_err(|error| StoreError::SchemaVersion(Box::new(error)))?;
        let versions: Vec<i64> = response
            .take(0)
            .map_err(|error| StoreError::SchemaVersion(Box::new(error)))?;
        Ok(versions.into_iter().next())
    }

    pub async fn initialise_schema(&self) -> Result<(), StoreError> {
        self.db
            .query(schema::BOOTSTRAP_QUERY)
            .await
            .map(|_| ())
            .map_err(|error| StoreError::Schema(Box::new(error)))
    }

    /// Executes a raw SurrealQL query against the underlying connection.
    pub fn query<'a>(
        &'a self,
        query: impl Into<std::borrow::Cow<'a, str>>,
    ) -> surrealdb::method::Query<'a, Any> {
        self.db.query(query)
    }
}
