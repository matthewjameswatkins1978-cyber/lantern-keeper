//! Fenced-export manifest coverage for the v1.1 hosted contract.
//!
//! The all-or-nothing hosted boundary needs a revision fence. Until a
//! monotonic fence counter exists, [`SurrealStore::export_fenced_to`]
//! records the operator-held fence in the manifest while plain
//! [`SurrealStore::export_to`] stays byte-identical to before.

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
    config.namespace = "export_fence".to_owned();
    config.database = database.to_owned();
    config.username.clear();
    config.password.clear();
    config
}

async fn seeded_store(label: &str) -> Result<(SurrealStore, PathBuf), Box<dyn std::error::Error>> {
    let db_path = test_path(label);
    let store = SurrealStore::connect(&embedded_config(db_path.clone(), "fence")).await?;
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
    SurrealMemoryRepository::new(store.clone())
        .store(Memory::new(NewMemory {
            content: "fence proof memory".to_owned(),
            kind: MemoryKind::Fact,
            project_id: None,
            confidence: 0.9,
            importance: 0.8,
            recorded_at: Utc::now(),
            known_at: Utc::now(),
            observed_at: None,
            valid_from: Utc::now(),
            valid_until: None,
            derived_from: vec!["source:fence-proof".to_owned()],
            updates: Vec::new(),
            extends: Vec::new(),
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            supports: Vec::new(),
            agent: "fence-test".to_owned(),
        })?)
        .await?;
    Ok((store, db_path))
}

fn read_manifest(dir: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest readable");
    serde_json::from_str(&text).expect("manifest is JSON")
}

#[tokio::test]
async fn unfenced_export_records_no_fence_key() -> Result<(), Box<dyn std::error::Error>> {
    let (store, db_path) = seeded_store("fence-unfenced").await?;
    let export_path = test_path("fence-unfenced-export");
    store.export_to(&export_path).await?;
    let manifest = read_manifest(&export_path);
    assert!(
        manifest.get("fenced").is_none(),
        "unfenced export must not gain a fenced key (byte-compat)"
    );
    assert!(
        manifest.get("fence_id").is_none(),
        "unfenced export must not gain a fence_id key"
    );
    let _ = std::fs::remove_dir_all(db_path);
    let _ = std::fs::remove_dir_all(export_path);
    Ok(())
}

#[tokio::test]
async fn fenced_export_records_fence_and_restores() -> Result<(), Box<dyn std::error::Error>> {
    let (store, db_path) = seeded_store("fence-fenced").await?;
    let export_path = test_path("fence-fenced-export");
    store
        .export_fenced_to(&export_path, "fence-test-001")
        .await?;
    let manifest = read_manifest(&export_path);
    assert_eq!(
        manifest.get("fenced"),
        Some(&serde_json::Value::Bool(true)),
        "fenced export must record fenced:true"
    );
    assert_eq!(
        manifest.get("fence_id").and_then(|v| v.as_str()),
        Some("fence-test-001"),
        "fenced export must record the caller fence id"
    );

    let restore_path = test_path("fence-restore");
    let restore_store =
        SurrealStore::connect(&embedded_config(restore_path.clone(), "restored")).await?;
    restore_store.initialise_schema().await?;
    SurrealSourceRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealMemoryRepository::new(restore_store.clone())
        .migrate()
        .await?;
    SurrealLedgerRepository::new(restore_store.clone())
        .migrate()
        .await?;
    restore_store.restore_from(&export_path).await?;
    let restored = restore_store
        .export_to(test_path("fence-restored-export"))
        .await?;
    assert_eq!(restored.record_count, 1);

    let _ = std::fs::remove_dir_all(db_path);
    let _ = std::fs::remove_dir_all(export_path);
    let _ = std::fs::remove_dir_all(restore_path);
    let _ = std::fs::remove_dir_all(restored.directory);
    Ok(())
}

#[tokio::test]
async fn restore_reports_fence_state() -> Result<(), Box<dyn std::error::Error>> {
    let (store, db_path) = seeded_store("fence-summary").await?;
    let unfenced_path = test_path("fence-summary-unfenced");
    store.export_to(&unfenced_path).await?;
    let unfenced = store.restore_from(&unfenced_path).await?;
    assert!(
        !unfenced.fenced,
        "unfenced restore must report fenced=false"
    );
    assert!(
        unfenced.fence_id.is_none(),
        "unfenced restore must report no fence id"
    );

    let fenced_path = test_path("fence-summary-fenced");
    store
        .export_fenced_to(&fenced_path, "fence-summary-001")
        .await?;
    let fenced = store.restore_from(&fenced_path).await?;
    assert!(fenced.fenced, "fenced restore must report fenced=true");
    assert_eq!(
        fenced.fence_id.as_deref(),
        Some("fence-summary-001"),
        "fenced restore must report the caller fence id"
    );

    let _ = std::fs::remove_dir_all(db_path);
    let _ = std::fs::remove_dir_all(unfenced_path);
    let _ = std::fs::remove_dir_all(fenced_path);
    Ok(())
}

#[tokio::test]
async fn truncated_export_fails_closed_on_hash_mismatch() -> Result<(), Box<dyn std::error::Error>>
{
    let (store, db_path) = seeded_store("fence-truncate").await?;
    let export_path = test_path("fence-truncate-export");
    store.export_to(&export_path).await?;

    // Truncate a data file: valid JSON prefix, missing tail. The manifest
    // hash no longer matches, so restore must refuse before writing anything.
    let mut biggest: Option<(PathBuf, Vec<u8>)> = None;
    for entry in std::fs::read_dir(&export_path)? {
        let entry = entry?;
        let candidate = entry.path();
        if candidate.extension().and_then(|ext| ext.to_str()) != Some("ndjson") {
            continue;
        }
        let bytes = std::fs::read(&candidate)?;
        let replace = match &biggest {
            Some((_, current)) => bytes.len() > current.len(),
            None => true,
        };
        if replace {
            biggest = Some((candidate, bytes));
        }
    }
    let (target, bytes) = biggest.expect("export produced no ndjson files");
    assert!(bytes.len() > 8, "largest export file too small to truncate");
    std::fs::write(&target, &bytes[..bytes.len() / 2])?;

    let err = match store.restore_from(&export_path).await {
        Ok(_) => panic!("truncated export restored without error"),
        Err(error) => error,
    };
    let message = error_to_string(&err);
    assert!(
        message.contains("hash mismatch"),
        "truncation must fail on the content hash, got: {message}"
    );

    let _ = std::fs::remove_dir_all(db_path);
    let _ = std::fs::remove_dir_all(export_path);
    Ok(())
}

fn error_to_string(error: &dyn std::error::Error) -> String {
    use std::fmt::Write as _;
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        let _ = write!(out, ": {next}");
        source = next.source();
    }
    out
}
