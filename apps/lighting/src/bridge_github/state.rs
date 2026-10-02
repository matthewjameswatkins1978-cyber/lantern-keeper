use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::schema::{Intent, Receipt, ReceiptStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessingPhase {
    Started,
    Applied,
    TerminalReceiptRecorded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub intent_id: String,
    pub intent_commit: String,
    pub canonical_digest: String,
    pub status: ReceiptStatus,
    pub phase: ProcessingPhase,
    pub receipt: Option<Receipt>,
    pub lantern_record_id: Option<String>,
    pub lantern_result_sha256: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BridgeStateData {
    pub last_processed_inbox_commit: Option<String>,
    pub idempotency: HashMap<String, IdempotencyRecord>,
    pub last_cycle_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_terminal_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("I/O error during state operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize/deserialize state JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct BridgeStateStore {
    path: PathBuf,
    pub data: BridgeStateData,
}

impl BridgeStateStore {
    pub fn load_or_create(path: &Path) -> Result<Self, StateError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if path.exists() {
            let mut file = File::open(path)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            let data: BridgeStateData = serde_json::from_str(&contents)?;
            Ok(Self {
                path: path.to_path_buf(),
                data,
            })
        } else {
            let store = Self {
                path: path.to_path_buf(),
                data: BridgeStateData::default(),
            };
            store.save()?;
            Ok(store)
        }
    }

    pub fn save(&self) -> Result<(), StateError> {
        let serialized = serde_json::to_string_pretty(&self.data)?;
        let tmp_path = self.path.with_extension("tmp");

        {
            let mut tmp_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            tmp_file.write_all(serialized.as_bytes())?;
            tmp_file.sync_all()?;
        }

        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    pub fn get_idempotency(&self, intent_id: &str) -> Option<&IdempotencyRecord> {
        self.data.idempotency.get(intent_id)
    }

    pub fn record_started(
        &mut self,
        intent: &Intent,
        commit: &str,
        canonical_digest: &str,
    ) -> Result<(), StateError> {
        self.data.idempotency.insert(
            intent.intent_id.clone(),
            IdempotencyRecord {
                intent_id: intent.intent_id.clone(),
                intent_commit: commit.to_string(),
                canonical_digest: canonical_digest.to_string(),
                status: ReceiptStatus::Indeterminate,
                phase: ProcessingPhase::Started,
                receipt: None,
                lantern_record_id: None,
                lantern_result_sha256: None,
                recorded_at: Utc::now(),
            },
        );
        self.save()
    }

    pub fn record_applied(
        &mut self,
        intent_id: &str,
        record_id: Option<&str>,
        result_sha256: Option<&str>,
        receipt: Receipt,
    ) -> Result<(), StateError> {
        if let Some(record) = self.data.idempotency.get_mut(intent_id) {
            record.status = ReceiptStatus::Applied;
            record.phase = ProcessingPhase::Applied;
            record.lantern_record_id = record_id.map(str::to_string);
            record.lantern_result_sha256 = result_sha256.map(str::to_string);
            record.receipt = Some(receipt);
            record.recorded_at = Utc::now();
        }
        self.save()
    }

    pub fn record_terminal(&mut self, intent_id: &str, receipt: Receipt) -> Result<(), StateError> {
        if let Some(record) = self.data.idempotency.get_mut(intent_id) {
            record.status = receipt.status;
            record.phase = ProcessingPhase::TerminalReceiptRecorded;
            record.receipt = Some(receipt);
            record.recorded_at = Utc::now();
        }
        self.save()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_record(
        &mut self,
        intent_id: &str,
        intent_commit: &str,
        canonical_digest: &str,
        status: ReceiptStatus,
        lantern_record_id: Option<String>,
        lantern_result_sha256: Option<String>,
        receipt: Option<Receipt>,
        recorded_at: DateTime<Utc>,
    ) {
        self.data
            .idempotency
            .entry(intent_id.to_string())
            .or_insert_with(|| IdempotencyRecord {
                intent_id: intent_id.to_string(),
                intent_commit: intent_commit.to_string(),
                canonical_digest: canonical_digest.to_string(),
                status,
                phase: if status == ReceiptStatus::Applied {
                    ProcessingPhase::Applied
                } else {
                    ProcessingPhase::TerminalReceiptRecorded
                },
                receipt,
                lantern_record_id,
                lantern_result_sha256,
                recorded_at,
            });
    }

    pub fn advance_checkpoint(&mut self, commit: &str) -> Result<(), StateError> {
        self.data.last_processed_inbox_commit = Some(commit.to_string());
        self.data.last_success_at = Some(Utc::now());
        self.save()
    }

    pub fn record_cycle(
        &mut self,
        success: bool,
        terminal_error: Option<String>,
    ) -> Result<(), StateError> {
        let now = Utc::now();
        self.data.last_cycle_at = Some(now);
        if success {
            self.data.last_success_at = Some(now);
            self.data.last_terminal_error = None;
        } else {
            self.data.last_terminal_error = terminal_error;
        }
        self.save()
    }

    pub fn get_last_receipt_outcome(&self) -> Option<String> {
        self.data
            .idempotency
            .values()
            .max_by_key(|r| r.recorded_at)
            .map(|r| {
                format!(
                    "{} (intent: {}, record: {})",
                    r.status.as_str(),
                    r.intent_id,
                    r.lantern_record_id.as_deref().unwrap_or("<none>")
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge_github::schema::{
        IntentAction, MirrorRefreshStatus, ReceiptAuthority, RequestedBy,
    };
    use uuid::Uuid;

    struct TestTempDir {
        path: PathBuf,
    }
    impl TestTempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("lantern_test_{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
        fn path(&self) -> &Path {
            &self.path
        }
    }
    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn state_save_and_reload() {
        let temp_dir = TestTempDir::new();
        let state_path = temp_dir.path().join("state.json");

        {
            let mut store = BridgeStateStore::load_or_create(&state_path).unwrap();
            store.advance_checkpoint("commit_abc123").unwrap();
            let intent = Intent {
                schema: "lantern.intent.v1".to_string(),
                intent_id: "01K999ABCDEF123456".to_string(),
                action: IntentAction::MemoryCreate,
                requested_by: RequestedBy {
                    actor: "test".to_string(),
                    transport: "github".to_string(),
                },
                created_at: Utc::now(),
                target: None,
                observed: None,
                payload: serde_json::json!({"content": "hi"}),
            };
            store
                .record_started(&intent, "commit_abc123", "sha256:1111")
                .unwrap();
        }

        {
            let store = BridgeStateStore::load_or_create(&state_path).unwrap();
            assert_eq!(
                store.data.last_processed_inbox_commit.as_deref(),
                Some("commit_abc123")
            );
            let item = store.get_idempotency("01K999ABCDEF123456").unwrap();
            assert_eq!(item.canonical_digest, "sha256:1111");
            assert_eq!(item.phase, ProcessingPhase::Started);
        }
    }

    #[test]
    fn crash_seam_applied_mutation_survives_restart() {
        let temp_dir = TestTempDir::new();
        let state_path = temp_dir.path().join("state.json");

        let receipt = Receipt {
            schema: "lantern.receipt.v1".to_string(),
            intent_id: "01K999ABCDEF123456".to_string(),
            intent_commit: "commit_abc123".to_string(),
            status: ReceiptStatus::Applied,
            authority: ReceiptAuthority {
                decision: "ALLOW".to_string(),
                reason: None,
            },
            lantern: None,
            processed_at: Utc::now(),
            bridge_version: "0.1.0".to_string(),
            mirror_refresh: MirrorRefreshStatus::NotRequested,
            error: None,
        };

        {
            let mut store = BridgeStateStore::load_or_create(&state_path).unwrap();
            let intent = Intent {
                schema: "lantern.intent.v1".to_string(),
                intent_id: "01K999ABCDEF123456".to_string(),
                action: IntentAction::MemoryCreate,
                requested_by: RequestedBy {
                    actor: "test".to_string(),
                    transport: "github".to_string(),
                },
                created_at: Utc::now(),
                target: None,
                observed: None,
                payload: serde_json::json!({"content": "hi"}),
            };
            store
                .record_started(&intent, "commit_abc123", "sha256:1111")
                .unwrap();
            // Simulate Lantern mutation succeeds and is recorded
            store
                .record_applied(
                    "01K999ABCDEF123456",
                    Some("record-1"),
                    Some("sha256:result"),
                    receipt.clone(),
                )
                .unwrap();
            // Process "crashes" here before checkpoint is advanced!
        }

        {
            // Process restarts: reload state
            let store = BridgeStateStore::load_or_create(&state_path).unwrap();
            let item = store.get_idempotency("01K999ABCDEF123456").unwrap();
            assert_eq!(item.phase, ProcessingPhase::Applied);
            assert_eq!(item.status, ReceiptStatus::Applied);
            assert_eq!(item.lantern_record_id.as_deref(), Some("record-1"));
            assert!(item.receipt.is_some());
        }
    }
}
