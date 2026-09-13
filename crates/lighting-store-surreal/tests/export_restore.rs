//! Recovery coverage for the engine-independent Lantern export.

use std::path::PathBuf;

use chrono::Utc;
use lighting_core::{Memory, MemoryKind, MemoryRepository, NewMemory};
use lighting_store_surreal::{
    StoreConfig, SurrealLedgerRepository, SurrealMemoryPathRepository, SurrealMemoryRepository,
    SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf, database: &str) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "export_restore".to_owned();
    config.database = database.to_owned();
    config.username.clear();
    config.password.clear();
    config
}

async fn initialise_store(store: &SurrealStore) -> Result<(), Box<dyn std::error::Error>> {
    store.initialise_schema().await?;
    SurrealSourceRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealMemoryRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealLedgerRepository::new(store.clone())
        .migrate()
        .await?;
    Ok(())
}

#[tokio::test]
async fn export_restore_replay_preserves_logical_count() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = test_path("export-source");
    let export_path = test_path("export");
    let restore_path = test_path("restore");

    let source_store =
        SurrealStore::connect(&embedded_config(source_path.clone(), "source")).await?;
    initialise_store(&source_store).await?;
    let repository = SurrealMemoryRepository::new(source_store.clone());
    repository
        .store(Memory::new(NewMemory {
            content: "restore proof memory".to_owned(),
            kind: MemoryKind::Fact,
            project_id: None,
            confidence: 0.9,
            importance: 0.8,
            recorded_at: Utc::now(),
            known_at: Utc::now(),
            observed_at: None,
            valid_from: Utc::now(),
            valid_until: None,
            derived_from: vec!["source:restore-proof".to_owned()],
            updates: Vec::new(),
            extends: Vec::new(),
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            supports: Vec::new(),
            agent: "restore-test".to_owned(),
        })?)
        .await?;
    let original = source_store.export_to(&export_path).await?;
    assert_eq!(original.record_count, 1);

    let restore_store =
        SurrealStore::connect(&embedded_config(restore_path.clone(), "restored")).await?;
    initialise_store(&restore_store).await?;
    restore_store.restore_from(&export_path).await?;
    restore_store.restore_from(&export_path).await?;
    let restored = restore_store
        .export_to(test_path("restored-export"))
        .await?;

    assert_eq!(restored.record_count, original.record_count);
    assert_eq!(restored.record_counts, original.record_counts);
    assert_eq!(restored.export_hash, original.export_hash);

    let _ = std::fs::remove_dir_all(source_path);
    let _ = std::fs::remove_dir_all(export_path);
    let _ = std::fs::remove_dir_all(restore_path);
    let _ = std::fs::remove_dir_all(restored.directory);
    Ok(())
}
