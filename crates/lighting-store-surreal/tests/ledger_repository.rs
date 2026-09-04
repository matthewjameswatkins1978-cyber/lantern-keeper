//! Replay and restart coverage for the append-only host-neutral ledger.

use std::{collections::BTreeMap, path::PathBuf};

use chrono::Utc;
use lighting_core::{
    LedgerEvent, LedgerEventRepository, LedgerIngestResult, LedgerRole,
};
use lighting_store_surreal::{StoreConfig, SurrealLedgerRepository, SurrealStore};
use uuid::Uuid;

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!("lantern-keeper-ledger-{}", Uuid::new_v4().simple()))
}

fn config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "ledger_repository".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

fn event(key: &str, received_at: chrono::DateTime<Utc>) -> LedgerEvent {
    LedgerEvent {
        event_id: format!("lucy:{key}"),
        source: "lucy".to_owned(),
        external_id: Some(key.to_owned()),
        session_id: Some("session-1".to_owned()),
        conversation_id: Some("conversation-1".to_owned()),
        turn_id: Some(key.to_owned()),
        actor: "matthew".to_owned(),
        role: LedgerRole::User,
        content: format!("A durable ledger event {key}."),
        observed_at: Some(received_at),
        received_at,
        reply_to: None,
        project_hint: Some("lantern".to_owned()),
        idempotency_key: key.to_owned(),
        raw_payload: Some(format!(r#"{{"id":"{key}"}}"#)),
        metadata: BTreeMap::from([(String::from("fixture"), String::from("qualification"))]),
    }
}

#[tokio::test]
async fn replay_is_idempotent_and_survives_reopen() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let first = event("event-1", Utc::now());
    let duplicate = first.clone();
    let second = event("event-2", Utc::now() + chrono::Duration::seconds(1));

    {
        let store = SurrealStore::connect(&config(path.clone())).await?;
        store.initialise_schema().await?;
        let repo = SurrealLedgerRepository::new(store);
        repo.migrate().await?;
        assert!(matches!(repo.ingest(first.clone()).await?, LedgerIngestResult::Stored(_)));
        assert!(matches!(repo.ingest(duplicate).await?, LedgerIngestResult::Duplicate(_)));
        assert!(matches!(repo.ingest(second).await?, LedgerIngestResult::Stored(_)));
        assert_eq!(repo.list(Some("lucy")).await?.len(), 2);
    }

    // SurrealKV closes its file-backed worker asynchronously after the last
    // handle drops. This wait makes the lifecycle boundary explicit and is
    // also part of the qualification evidence for Windows file locking.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    {
        let store = SurrealStore::connect(&config(path.clone())).await?;
        store.initialise_schema().await?;
        let repo = SurrealLedgerRepository::new(store);
        repo.migrate().await?;
        let events = repo.list(Some("lucy")).await?;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "lucy:event-1");
        assert_eq!(events[0].raw_payload, Some(r#"{"id":"event-1"}"#.to_owned()));
    }

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
