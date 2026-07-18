use std::env;

use serde::Serialize;
use thiserror::Error;

const DEFAULT_ENDPOINT: &str = "ws://127.0.0.1:8000";
const DEFAULT_NAMESPACE: &str = "lantern_keeper";
const DEFAULT_DATABASE: &str = "lighting_dev";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreConfig {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RedactedStoreConfig {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("LIGHTING_SURREAL_ENDPOINT cannot be empty")]
    EmptyEndpoint,
    #[error("LIGHTING_SURREAL_NAMESPACE cannot be empty")]
    EmptyNamespace,
    #[error("LIGHTING_SURREAL_DATABASE cannot be empty")]
    EmptyDatabase,
    #[error("LIGHTING_SURREAL_USERNAME and LIGHTING_SURREAL_PASSWORD must be set together")]
    IncompleteCredentials,
}

impl StoreConfig {
    pub fn from_env() -> Self {
        Self {
            endpoint: env_or_default("LIGHTING_SURREAL_ENDPOINT", DEFAULT_ENDPOINT),
            namespace: env_or_default("LIGHTING_SURREAL_NAMESPACE", DEFAULT_NAMESPACE),
            database: env_or_default("LIGHTING_SURREAL_DATABASE", DEFAULT_DATABASE),
            username: env::var("LIGHTING_SURREAL_USERNAME").unwrap_or_default(),
            password: env::var("LIGHTING_SURREAL_PASSWORD").unwrap_or_default(),
        }
    }

    pub fn redacted(&self) -> RedactedStoreConfig {
        RedactedStoreConfig {
            endpoint: self.endpoint.clone(),
            namespace: self.namespace.clone(),
            database: self.database.clone(),
            username: self.username.clone(),
            password: "[redacted]".to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.endpoint.trim().is_empty() {
            return Err(ConfigError::EmptyEndpoint);
        }

        if self.namespace.trim().is_empty() {
            return Err(ConfigError::EmptyNamespace);
        }

        if self.database.trim().is_empty() {
            return Err(ConfigError::EmptyDatabase);
        }

        if self.username.is_empty() != self.password.is_empty() {
            return Err(ConfigError::IncompleteCredentials);
        }

        Ok(())
    }

    pub fn websocket_address(&self) -> &str {
        self.endpoint
            .strip_prefix("ws://")
            .or_else(|| self.endpoint.strip_prefix("wss://"))
            .unwrap_or(&self.endpoint)
    }
}

fn env_or_default(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn clear_env() {
        env::remove_var("LIGHTING_SURREAL_ENDPOINT");
        env::remove_var("LIGHTING_SURREAL_NAMESPACE");
        env::remove_var("LIGHTING_SURREAL_DATABASE");
        env::remove_var("LIGHTING_SURREAL_USERNAME");
        env::remove_var("LIGHTING_SURREAL_PASSWORD");
    }

    #[test]
    fn loads_safe_local_defaults() {
        let _guard = env_lock();
        clear_env();

        let config = StoreConfig::from_env();

        assert_eq!(config.endpoint, "ws://127.0.0.1:8000");
        assert_eq!(config.namespace, "lantern_keeper");
        assert_eq!(config.database, "lighting_dev");
        assert_eq!(config.username, "");
        assert_eq!(config.password, "");
    }

    #[test]
    fn redacts_password_completely() {
        let config = StoreConfig {
            endpoint: "ws://127.0.0.1:8000".to_owned(),
            namespace: "lantern_keeper".to_owned(),
            database: "lighting_dev".to_owned(),
            username: "root".to_owned(),
            password: "root".to_owned(),
        };

        let redacted = config.redacted();

        assert_eq!(redacted.password, "[redacted]");
    }

    #[test]
    fn validates_required_fields() {
        let mut config = StoreConfig::from_env();

        config.endpoint = " ".to_owned();
        assert_eq!(config.validate(), Err(ConfigError::EmptyEndpoint));

        config.endpoint = "ws://127.0.0.1:8000".to_owned();
        config.namespace.clear();
        assert_eq!(config.validate(), Err(ConfigError::EmptyNamespace));

        config.namespace = "lantern_keeper".to_owned();
        config.database.clear();
        assert_eq!(config.validate(), Err(ConfigError::EmptyDatabase));
    }

    #[test]
    fn validates_credentials_are_complete() {
        let mut config = StoreConfig::from_env();
        config.username = "root".to_owned();
        config.password.clear();

        assert_eq!(config.validate(), Err(ConfigError::IncompleteCredentials));
    }
}
