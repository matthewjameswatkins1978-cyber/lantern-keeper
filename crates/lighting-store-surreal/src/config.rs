use std::env;

use serde::Serialize;

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
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
}
