//! Workload-oriented qualification for the embedded SurrealKV default.

use std::{path::PathBuf, time::Duration};

use chrono::Utc;
use lighting_store_surreal::{StoreConfig, SurrealStore};
use surrealdb::types::Object;
use tokio::task::JoinSet;
use uuid::Uuid;

fn qualification_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-surrealkv-qualification-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "qualification".to_owned();
    config.database = "workload".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

async fn value(store: &SurrealStore) -> Result<String, Box<dyn std::error::Error>> {
    let record: Option<Object> = store
        .query("SELECT payload FROM qualification WHERE id = type::record('qualification', 'one')")
        .await?
        .take(0)?;
    record
        .and_then(|object| object.get("payload").cloned())
        .and_then(|value| value.into_t::<String>().ok())
        .ok_or_else(|| "qualification record had no value".into())
}

#[tokio::test]
async fn embedded_surrealkv_survives_core_workload() -> Result<(), Box<dyn std::error::Error>> {
    let path = qualification_path();
    let config = embedded_config(path.clone());

    let store = SurrealStore::connect(&config).await?;
    store
        .query(
            "DEFINE TABLE IF NOT EXISTS qualification SCHEMAFULL;
             DEFINE FIELD IF NOT EXISTS bucket ON qualification TYPE string;
             DEFINE FIELD IF NOT EXISTS payload ON qualification TYPE string;
             DEFINE FIELD IF NOT EXISTS updated_at ON qualification TYPE datetime;
             DEFINE INDEX IF NOT EXISTS qualification_bucket ON qualification FIELDS bucket;",
        )
        .await?;

    store
        .query(
            "CREATE qualification:one CONTENT {
                bucket: 'versioned', payload: 'v1', updated_at: time::now()
            };",
        )
        .await?;
    let version = Utc::now().to_rfc3339();

    store
        .query(
            "UPDATE qualification SET payload = 'v2', updated_at = time::now() WHERE id = type::record('qualification', 'one');",
        )
        .await?;
    assert_eq!(value(&store).await?, "v2");

    let historical: Option<Object> = store
        .query(format!(
            "SELECT payload FROM qualification:one VERSION d'{}'",
            version
        ))
        .await?
        .take(0)?;
    let historical_value = historical
        .and_then(|object| object.get("payload").cloned())
        .and_then(|value| value.into_t::<String>().ok())
        .ok_or("historical version was not returned")?;
    assert_eq!(historical_value, "v1");

    let mut writers = JoinSet::new();
    for worker in 0..4 {
        let worker_store = store.clone();
        writers.spawn(async move {
            for item in 0..8 {
                worker_store
                    .query(
                        "CREATE qualification CONTENT {
                            bucket: 'concurrent', payload: $payload, updated_at: time::now()
                        };",
                    )
                    .bind(("payload", format!("worker-{worker}-{item}")))
                    .await
                    .map(|_| ())?;
            }
            Ok::<(), surrealdb::Error>(())
        });
    }
    while let Some(result) = writers.join_next().await {
        result??;
    }

    let concurrent: Vec<Object> = store
        .query("SELECT * FROM qualification WHERE bucket = 'concurrent'")
        .await?
        .take(0)?;
    assert_eq!(concurrent.len(), 32);

    drop(store);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let reopened = SurrealStore::connect(&config).await?;
    assert_eq!(value(&reopened).await?, "v2");
    let persisted: Vec<Object> = reopened
        .query("SELECT * FROM qualification WHERE bucket = 'concurrent'")
        .await?
        .take(0)?;
    assert_eq!(persisted.len(), 32);

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
