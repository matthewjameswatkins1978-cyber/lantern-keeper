//! Hostile experiment harness; emits facts, deliberately never asserts product invariants.
use chrono::Utc;
use lighting_core::{LedgerEvent, LedgerEventRepository, LedgerIngestResult, LedgerRole};
use lighting_store_surreal::{StoreConfig, SurrealLedgerRepository, SurrealStore};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::Barrier;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emit_history() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::var("TB_LANTERN_SCENARIO").unwrap_or_else(|_| "race".into());
    let count: usize = std::env::var("TB_LANTERN_WRITERS")
        .unwrap_or_else(|_| "16".into())
        .parse()?;
    if !(1..=32).contains(&count) {
        return Err("writers outside 1..32".into());
    }
    let path = std::env::temp_dir().join(format!("tb-lantern-store-{}", Uuid::new_v4()));
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".into();
    config.path = path.clone();
    config.namespace = "terrorbat_fixture".into();
    config.database = "disposable".into();
    config.username.clear();
    config.password.clear();
    let base = LedgerEvent {
        event_id: "terrorbat:one".into(),
        source: "terrorbat".into(),
        external_id: None,
        session_id: None,
        conversation_id: None,
        turn_id: None,
        actor: "fixture".into(),
        role: LedgerRole::User,
        content: "original fixture payload".into(),
        observed_at: None,
        received_at: Utc::now(),
        reply_to: None,
        project_hint: None,
        idempotency_key: "terrorbat-key".into(),
        raw_payload: None,
        metadata: BTreeMap::new(),
    };
    let mut history = Vec::new();
    {
        let store = SurrealStore::connect(&config).await?;
        store.initialise_schema().await?;
        let repo = SurrealLedgerRepository::new(store);
        repo.migrate().await?;
        if mode == "race" {
            let barrier = Arc::new(Barrier::new(count));
            let mut handles = Vec::new();
            for writer in 0..count {
                let repo = repo.clone();
                let event = base.clone();
                let barrier = barrier.clone();
                handles.push(tokio::spawn(async move {
                    barrier.wait().await;
                    let outcome = repo.ingest(event).await;
                    match outcome {
                        Ok(LedgerIngestResult::Stored(record)) => {
                            json!({"writer":writer,"status":"stored","record":record})
                        }
                        Ok(LedgerIngestResult::Duplicate(record)) => {
                            json!({"writer":writer,"status":"duplicate","record":record})
                        }
                        Err(error) => {
                            json!({"writer":writer,"status":"error","error":error.to_string()})
                        }
                    }
                }));
            }
            for handle in handles {
                history.push(handle.await?);
            }
        } else {
            for writer in 0..count {
                let mut event = base.clone();
                if mode == "different" && writer > 0 {
                    event.content = "materially different fixture payload".into();
                }
                let requested = event.content.clone();
                let outcome = repo.ingest(event).await;
                history.push(match outcome {
                    Ok(LedgerIngestResult::Stored(record)) => json!({"writer":writer,"status":"stored","requested_content":requested,"record":record}),
                    Ok(LedgerIngestResult::Duplicate(record)) => json!({"writer":writer,"status":"duplicate","requested_content":requested,"record":record}),
                    Err(error) => json!({"writer":writer,"status":"error","error":error.to_string()}),
                });
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    let durable = {
        let store = SurrealStore::connect(&config).await?;
        store.initialise_schema().await?;
        let repo = SurrealLedgerRepository::new(store);
        repo.migrate().await?;
        repo.list(Some("terrorbat")).await?
    };
    println!(
        "TB_HISTORY={}",
        json!({"schema":"lantern-hostile-history/v1","scenario":mode,"writers":count,"history":history,"closed_and_reopened":true,"durable_records":durable})
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::remove_dir_all(path)?;
    Ok(())
}
