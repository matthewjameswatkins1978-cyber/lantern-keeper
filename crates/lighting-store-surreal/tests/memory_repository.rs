//! Integration coverage for the first durable Living Memory loop.

use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use lighting_core::{Memory, MemoryKind, MemoryRepository, MemorySearchQuery, NewMemory};
use lighting_store_surreal::{StoreConfig, SurrealMemoryRepository, SurrealStore};
use lighting_store_surreal::{SurrealMemoryPathRepository, SurrealSourceRepository};
use uuid::Uuid;

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-memory-repository-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "memory_repository".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

#[tokio::test]
async fn stores_recalls_and_supersedes_memory_without_losing_history()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let config = embedded_config(path.clone());
    let store = SurrealStore::connect(&config).await?;
    store.initialise_schema().await?;
    SurrealSourceRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(store.clone())
        .migrate()
        .await?;
    let repository = SurrealMemoryRepository::new(store.clone());
    repository.migrate().await?;

    let now = Utc::now();
    let memory = Memory::new(NewMemory {
        content: "Matthew prefers evidence-linked, concise working context.".to_owned(),
        kind: MemoryKind::Preference,
        project_id: None,
        confidence: 0.95,
        importance: 0.9,
        recorded_at: now,
        known_at: now,
        observed_at: Some(now),
        valid_from: now,
        valid_until: None,
        derived_from: vec!["source:conversation-001".to_owned()],
        updates: Vec::new(),
        extends: Vec::new(),
        supersedes: Vec::new(),
        contradicts: Vec::new(),
        supports: Vec::new(),
        agent: "lucy".to_owned(),
    })?;
    let memory_id = memory.id.clone();

    repository.store(memory.clone()).await?;
    assert_eq!(repository.get(&memory_id).await?, Some(memory));

    let matches = repository
        .search(&MemorySearchQuery {
            phrase: Some("evidence-linked".to_owned()),
            ..Default::default()
        })
        .await?;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, memory_id);

    let superseded = repository.supersede(&memory_id, Utc::now()).await?;
    assert_eq!(superseded.status, lighting_core::MemoryStatus::Superseded);
    assert!(
        repository
            .search(&MemorySearchQuery {
                phrase: Some("evidence-linked".to_owned()),
                ..Default::default()
            })
            .await?
            .is_empty()
    );

    let historical = repository
        .search(&MemorySearchQuery {
            phrase: Some("evidence-linked".to_owned()),
            include_inactive: true,
            ..Default::default()
        })
        .await?;
    assert_eq!(historical.len(), 1);
    assert_eq!(
        historical[0].derived_from,
        vec!["source:conversation-001".to_owned()]
    );

    let export_path = test_path();
    let export = store.export_to(&export_path).await?;
    assert_eq!(export.record_count, 1);
    let manifest = std::fs::read_to_string(export_path.join("manifest.json"))?;
    assert!(manifest.contains("lantern-keeper-export-v1"));

    let _ = std::fs::remove_dir_all(path);
    let _ = std::fs::remove_dir_all(export_path);
    Ok(())
}

#[tokio::test]
async fn historical_queries_and_lineage_preserve_evolution() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let config = embedded_config(path.clone());
    let store = SurrealStore::connect(&config).await?;
    store.initialise_schema().await?;
    let repository = SurrealMemoryRepository::new(store.clone());
    repository.migrate().await?;

    let t1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let old = Memory::new(NewMemory {
        content: "The project is called Suture.".to_owned(),
        kind: MemoryKind::Decision,
        project_id: None,
        confidence: 0.9,
        importance: 0.8,
        recorded_at: t1,
        known_at: t1,
        observed_at: Some(t1),
        valid_from: t1,
        valid_until: Some(t2),
        derived_from: vec!["source:rename-1".to_owned()],
        updates: Vec::new(),
        extends: Vec::new(),
        supersedes: Vec::new(),
        contradicts: Vec::new(),
        supports: Vec::new(),
        agent: "lucy".to_owned(),
    })?;
    let old_id = old.id.clone();
    let current = Memory::new(NewMemory {
        content: "The project is called Threadmoth.".to_owned(),
        kind: MemoryKind::Decision,
        project_id: None,
        confidence: 0.95,
        importance: 0.9,
        recorded_at: t2,
        known_at: t2,
        observed_at: Some(t2),
        valid_from: t2,
        valid_until: None,
        derived_from: vec!["source:rename-2".to_owned()],
        updates: vec![old_id.to_string()],
        extends: Vec::new(),
        supersedes: vec![old_id.to_string()],
        contradicts: Vec::new(),
        supports: Vec::new(),
        agent: "lucy".to_owned(),
    })?;
    let current_id = current.id.clone();
    repository.store(old).await?;
    repository.store(current).await?;

    let historical = repository
        .search(&MemorySearchQuery {
            phrase: Some("project is called".to_owned()),
            as_of: Some(t1 + chrono::Duration::days(10)),
            ..Default::default()
        })
        .await?;
    assert_eq!(historical.len(), 1);
    assert!(historical[0].content.contains("Suture"));

    let present = repository
        .search(&MemorySearchQuery {
            phrase: Some("project is called".to_owned()),
            as_of: Some(t2 + chrono::Duration::days(10)),
            ..Default::default()
        })
        .await?;
    assert_eq!(present.len(), 1);
    assert_eq!(present[0].id, current_id);

    let lineage = repository.lineage(&current_id, 4).await?;
    assert_eq!(lineage.len(), 2);
    assert_eq!(lineage[0].id, current_id);
    assert_eq!(lineage[1].id, old_id);

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
