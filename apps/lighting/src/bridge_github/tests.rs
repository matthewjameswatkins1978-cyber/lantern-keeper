use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;

use super::executor::BridgeExecutor;
use super::queue::{BridgeStatusFile, DiscoveredIntentCommit, GitQueue, QueueError};
use super::schema::ReceiptStatus;
use super::state::BridgeStateStore;
use super::tethers::TethersGate;

struct TestTempDir {
    path: PathBuf,
}

impl TestTempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lantern_bridge_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("failed to create test temp dir");
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

fn run_git_cmd(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute git {:?}: {e}", args));

    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_test_git_repo(path: &Path) {
    run_git_cmd(path, &["init"]);
    run_git_cmd(path, &["config", "user.name", "TestRunner"]);
    run_git_cmd(path, &["config", "user.email", "test@example.com"]);
    run_git_cmd(path, &["config", "commit.gpgsign", "false"]);
    run_git_cmd(path, &["checkout", "-b", "inbox"]);
    run_git_cmd(path, &["commit", "--allow-empty", "-m", "initial commit"]);
}

#[test]
fn test_queue_ancestry_verification_and_suspect_on_rewritten_history() {
    let temp = TestTempDir::new();
    init_test_git_repo(temp.path());

    let c0 = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    // Commit 1 on inbox
    let intents_dir = temp.path().join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();
    std::fs::write(intents_dir.join("01K001.json"), "{}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K001.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "intent 1"]);
    let c1 = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    let queue = GitQueue::new(temp.path());

    // Ancestry check: c0 is ancestor of c1
    assert!(queue.is_ancestor(&c0, &c1).unwrap());
    let commits = queue.list_commits_between(Some(&c0), &c1).unwrap();
    assert_eq!(commits, vec![c1.clone()]);

    // Create a diverged history from c0 to simulate rewritten/replaced history
    run_git_cmd(temp.path(), &["checkout", &c0]);
    run_git_cmd(temp.path(), &["checkout", "-b", "rewritten"]);
    std::fs::create_dir_all(&intents_dir).unwrap();
    std::fs::write(intents_dir.join("01K_rogue.json"), "{}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K_rogue.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "rogue intent"]);
    let c_rogue = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    // If checkpoint was c1, but head became c_rogue:
    // c1 is NOT an ancestor of c_rogue!
    assert!(!queue.is_ancestor(&c1, &c_rogue).unwrap());

    // list_commits_between MUST fail closed with NonDescendantInboxHead
    let err = queue.list_commits_between(Some(&c1), &c_rogue).unwrap_err();
    match err {
        QueueError::NonDescendantInboxHead {
            checkpoint,
            inbox_head,
        } => {
            assert_eq!(checkpoint, c1);
            assert_eq!(inbox_head, c_rogue);
        }
        other => panic!("expected NonDescendantInboxHead, got: {:?}", other),
    }
}

#[test]
fn test_queue_modifying_intent_is_suspect() {
    let temp = TestTempDir::new();
    init_test_git_repo(temp.path());

    let intents_dir = temp.path().join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();
    let intent_file = intents_dir.join("01K001.json");

    // Add intent file in commit 1
    std::fs::write(&intent_file, "{\"schema\":\"lantern.intent.v1\"}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K001.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "add intent"]);

    // Modify intent file in commit 2 (FORBIDDEN)
    std::fs::write(&intent_file, "{\"schema\":\"tampered\"}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K001.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "modify intent"]);
    let modify_commit = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    let queue = GitQueue::new(temp.path());
    let err = queue
        .extract_intent_from_commit(&modify_commit)
        .unwrap_err();

    match err {
        QueueError::ForbiddenModification { commit, path } => {
            assert_eq!(commit, modify_commit);
            assert_eq!(path, "intents/01K001.json");
        }
        other => panic!("expected ForbiddenModification, got: {:?}", other),
    }
}

#[test]
fn test_queue_oldest_first_ordering() {
    let temp = TestTempDir::new();
    init_test_git_repo(temp.path());

    let c0 = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    let intents_dir = temp.path().join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();

    // Commit A
    std::fs::write(intents_dir.join("01K_A.json"), "{\"id\":\"A\"}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K_A.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "commit A"]);
    let ca = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    // Commit B
    std::fs::write(intents_dir.join("01K_B.json"), "{\"id\":\"B\"}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K_B.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "commit B"]);
    let cb = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    // Commit C
    std::fs::write(intents_dir.join("01K_C.json"), "{\"id\":\"C\"}").unwrap();
    run_git_cmd(temp.path(), &["add", "intents/01K_C.json"]);
    run_git_cmd(temp.path(), &["commit", "-m", "commit C"]);
    let cc = run_git_cmd(temp.path(), &["rev-parse", "HEAD"]);

    let queue = GitQueue::new(temp.path());
    let commits = queue.list_commits_between(Some(&c0), &cc).unwrap();

    // Must be in topological oldest-first order: [A, B, C]
    assert_eq!(commits, vec![ca, cb, cc]);
}

#[tokio::test]
async fn test_executor_tethers_allow_ask_deny_unavailable() {
    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();

    // Start mock Tethers/Lantern HTTP server
    let app = Router::new()
        .route(
            "/api/v1/tethers/authority/check",
            post(|Json(req): Json<serde_json::Value>| async move {
                let principal = req
                    .get("principal_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match principal {
                    "agent:actor-allow" => Json(json!({
                        "decision": "ALLOW",
                        "grant_id": "grant_test_123"
                    })),
                    "agent:actor-ask" => Json(json!({
                        "decision": "ASK",
                        "reason": "Requires human operator approval"
                    })),
                    "agent:actor-deny" => Json(json!({
                        "decision": "DENY",
                        "reason": "Principal lacks capability"
                    })),
                    _ => Json(json!({
                        "decision": "DENY",
                        "reason": "Unknown principal"
                    })),
                }
            }),
        )
        .route(
            "/api/v1/memories",
            post(|| async {
                Json(json!({
                    "id": "019323ef-6258-75b2-a42e-13c2f0fcf6d6",
                    "hash": "sha256:mem123"
                }))
            }),
        )
        .route(
            "/api/v1/bridge/intents/claim",
            post(|Json(req): Json<serde_json::Value>| async move {
                Json(json!({
                    "outcome": "claimed",
                    "record": {
                        "intent_id": req.get("intent_id").unwrap_or(&json!("")),
                        "status": "CLAIMED",
                    }
                }))
            }),
        )
        .route(
            "/api/v1/bridge/intents/complete",
            post(
                |Json(req): Json<serde_json::Value>| async move { Json(json!({ "intent": req })) },
            ),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tethers = TethersGate::new(server_url.clone(), None, None);
    let executor = BridgeExecutor::new(server_url, tethers, None);

    // 1. Tethers ALLOW -> Mutation proceeds to Applied
    let intent_allow = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999TESTALLOW12345",
        "action": "memory.create",
        "requested_by": { "actor": "actor-allow", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Memory to create under ALLOW" }
    });
    let discovered_allow = DiscoveredIntentCommit {
        commit_sha: "commit_allow_1".to_string(),
        intent_file_path: "intents/01K999TESTALLOW12345.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_allow).unwrap(),
    };
    let receipt_allow = executor
        .process_discovered_intent(&discovered_allow, &mut state, None)
        .await;
    assert_eq!(receipt_allow.status, ReceiptStatus::Applied);
    assert_eq!(receipt_allow.authority.decision, "ALLOW");

    // 2. Tethers ASK -> Receipt REQUIRES_APPROVAL
    let intent_ask = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999TESTASK12345678",
        "action": "memory.create",
        "requested_by": { "actor": "actor-ask", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Memory to create under ASK" }
    });
    let discovered_ask = DiscoveredIntentCommit {
        commit_sha: "commit_ask_1".to_string(),
        intent_file_path: "intents/01K999TESTASK12345678.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_ask).unwrap(),
    };
    let receipt_ask = executor
        .process_discovered_intent(&discovered_ask, &mut state, None)
        .await;
    assert_eq!(receipt_ask.status, ReceiptStatus::RequiresApproval);
    assert_eq!(receipt_ask.authority.decision, "ASK");
    assert!(
        receipt_ask
            .authority
            .reason
            .unwrap()
            .contains("Requires human operator approval")
    );

    // 3. Tethers DENY -> Receipt DENIED
    let intent_deny = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999TESTDENY1234567",
        "action": "memory.create",
        "requested_by": { "actor": "actor-deny", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Memory to create under DENY" }
    });
    let discovered_deny = DiscoveredIntentCommit {
        commit_sha: "commit_deny_1".to_string(),
        intent_file_path: "intents/01K999TESTDENY1234567.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_deny).unwrap(),
    };
    let receipt_deny = executor
        .process_discovered_intent(&discovered_deny, &mut state, None)
        .await;
    assert_eq!(receipt_deny.status, ReceiptStatus::Denied);
    assert_eq!(receipt_deny.authority.decision, "DENY");

    // 4. Tethers Unavailable -> Fails closed with DENIED and fail-closed reason
    let dead_tethers = TethersGate::new("http://127.0.0.1:54321".to_string(), None, None);
    let dead_executor =
        BridgeExecutor::new("http://127.0.0.1:54321".to_string(), dead_tethers, None);
    let intent_unavail = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999TESTUNAVAIL1234",
        "action": "memory.create",
        "requested_by": { "actor": "actor-allow", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Memory when tethers down" }
    });
    let discovered_unavail = DiscoveredIntentCommit {
        commit_sha: "commit_unavail_1".to_string(),
        intent_file_path: "intents/01K999TESTUNAVAIL1234.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_unavail).unwrap(),
    };
    let receipt_unavail = dead_executor
        .process_discovered_intent(&discovered_unavail, &mut state, None)
        .await;
    assert_eq!(receipt_unavail.status, ReceiptStatus::Denied);
    assert_eq!(receipt_unavail.authority.decision, "DENY");
    assert!(
        receipt_unavail
            .error
            .unwrap()
            .contains("tethers_unavailable_fail_closed")
    );
}

#[tokio::test]
async fn test_executor_observed_hash_precondition_conflict() {
    use lighting_core::{MemoryRepository, MemorySearchQuery};
    use lighting_service::{AppState, BridgeIntentService, MemoryService, build_router};
    use lighting_store_surreal::{
        StoreConfig, SurrealBridgeIntentRepository, SurrealMemoryRepository, SurrealStore,
    };

    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_string();
    config.path = temp.path().join("store");
    config.namespace = "bridge_observed_hash".to_string();
    config.database = "durability".to_string();
    config.username.clear();
    config.password.clear();

    // Use the real memory and bridge-intent HTTP routes backed by SurrealKV.
    let store = SurrealStore::connect(&config).await.unwrap();
    store.initialise_schema().await.unwrap();
    let memory_repo = Arc::new(SurrealMemoryRepository::new(store.clone()));
    memory_repo.migrate().await.unwrap();
    let bridge_repo = Arc::new(SurrealBridgeIntentRepository::new(store.clone()));
    bridge_repo.migrate().await.unwrap();
    let memory_service = MemoryService::new(memory_repo.clone());
    let original = memory_service
        .remember(
            serde_json::from_value(json!({
                "content": "Original live content", "kind": "fact", "agent": "test"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let mut app_state = AppState::new_unready();
    app_state.memory_service = Some(memory_service);
    app_state.bridge_intent_service = Some(BridgeIntentService::new(bridge_repo.clone()));
    app_state.mark_ready();
    let app = build_router(app_state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_url = format!("http://{}", listener.local_addr().unwrap());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    // Only authority is mocked; all target reads and possible mutations use the store.
    let authority = Router::new().route(
        "/api/v1/tethers/authority/check",
        post(|| async { Json(json!({ "decision": "ALLOW", "grant_id": "grant_ok" })) }),
    );
    let authority_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let authority_url = format!("http://{}", authority_listener.local_addr().unwrap());
    let authority_server = tokio::spawn(async move {
        axum::serve(authority_listener, authority).await.unwrap();
    });
    let tethers = TethersGate::new(authority_url, None, None);
    let executor = BridgeExecutor::new(server_url, tethers, None);
    let intent_conflict = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999CONFLICT1234567",
        "action": "memory.archive",
        "requested_by": { "actor": "client", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "target": { "record_id": original.id.as_str() },
        "observed": {
            "record_sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        },
        "payload": { "reason": "Archiving outdated memory" }
    });
    let discovered = DiscoveredIntentCommit {
        commit_sha: "commit_conflict_1".to_string(),
        intent_file_path: "intents/01K999CONFLICT1234567.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_conflict).unwrap(),
    };
    let receipt = executor
        .process_discovered_intent(&discovered, &mut state, None)
        .await;
    assert_eq!(receipt.status, ReceiptStatus::Conflict);
    assert!(receipt.error.unwrap().contains("record hash mismatch"));

    drop(executor);
    shutdown_tx.send(()).unwrap();
    server.await.unwrap();
    authority_server.abort();
    let _ = authority_server.await;
    drop(memory_repo);
    drop(bridge_repo);
    drop(store);
    // SurrealKV closes its file-backed worker asynchronously on Windows.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let reopened = SurrealStore::connect(&config).await.unwrap();
    reopened.initialise_schema().await.unwrap();
    let repo = SurrealMemoryRepository::new(reopened);
    repo.migrate().await.unwrap();
    let durable = repo.get(&original.id).await.unwrap().unwrap();
    assert_eq!(
        durable, original,
        "stale observed hash must leave every target field unchanged"
    );
    let memories = repo
        .search(&MemorySearchQuery {
            project_id: None,
            phrase: None,
            include_inactive: true,
            as_of: None,
        })
        .await
        .unwrap();
    assert_eq!(
        memories.len(),
        1,
        "conflict must not create replacement memory"
    );
    drop(repo);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_executor_idempotency_crash_seam_and_tamper() {
    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();

    let server_called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_called_clone = Arc::clone(&server_called);

    let canonical_intents: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
    > = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let canonical_claim = Arc::clone(&canonical_intents);
    let canonical_complete = Arc::clone(&canonical_intents);

    let app = Router::new()
        .route(
            "/api/v1/tethers/authority/check",
            post(|| async {
                Json(json!({
                    "decision": "ALLOW",
                    "grant_id": "grant_idemp"
                }))
            }),
        )
        .route(
            "/api/v1/memories",
            post(move || {
                let count = Arc::clone(&server_called_clone);
                async move {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Json(json!({
                        "id": "019323ef-6258-75b2-a42e-13c2f0fcf6d6",
                        "content": "Original idempotent memory",
                        "status": "active"
                    }))
                }
            }),
        )
        .route(
            "/api/v1/bridge/intents/claim",
            post(move |Json(req): Json<serde_json::Value>| {
                let store = Arc::clone(&canonical_claim);
                async move {
                    let intent_id = req["intent_id"].as_str().unwrap().to_string();
                    let canonical_digest = req["canonical_digest"].as_str().unwrap().to_string();
                    let mut map = store.lock().await;
                    if let Some(existing) = map.get(&intent_id) {
                        let existing_digest = existing["canonical_digest"].as_str().unwrap();
                        if existing_digest != canonical_digest {
                            // Digest mismatch on replay -> tamper detected
                            return Json(json!({
                                "outcome": "conflict",
                                "existing_digest": existing_digest,
                                "incoming_digest": canonical_digest
                            }));
                        }
                        Json(json!({ "outcome": "existing", "record": existing }))
                    } else {
                        map.insert(intent_id, req.clone());
                        Json(json!({ "outcome": "claimed", "record": req }))
                    }
                }
            }),
        )
        .route(
            "/api/v1/bridge/intents/complete",
            post(move |Json(req): Json<serde_json::Value>| {
                let store = Arc::clone(&canonical_complete);
                async move {
                    let intent_id = req["intent_id"].as_str().unwrap().to_string();
                    let mut map = store.lock().await;
                    if let Some(rec) = map.get_mut(&intent_id) {
                        rec["status"] = json!(req["status"].as_str().unwrap());
                        rec["lantern_record_id"] = req["lantern_record_id"].clone();
                        rec["result_digest"] = req["result_digest"].clone();
                    }
                    Json(json!({ "intent": req }))
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tethers = TethersGate::new(server_url.clone(), None, None);
    let executor = BridgeExecutor::new(server_url, tethers, None);

    let intent_orig = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999IDEMP0000000001",
        "action": "memory.create",
        "requested_by": { "actor": "client", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Original idempotent memory" }
    });

    let discovered = DiscoveredIntentCommit {
        commit_sha: "commit_idemp_1".to_string(),
        intent_file_path: "intents/01K999IDEMP0000000001.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_orig).unwrap(),
    };

    // First execution: mutates Lantern and stores in canonical datastore
    let receipt1 = executor
        .process_discovered_intent(&discovered, &mut state, None)
        .await;
    assert_eq!(receipt1.status, ReceiptStatus::Applied);
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint must be called once"
    );

    // Simulate crash after mutation before receipt was pushed.
    // On reboot/replay of the same intent with surviving local state:
    let receipt2 = executor
        .process_discovered_intent(&discovered, &mut state, None)
        .await;
    assert_eq!(receipt2.status, ReceiptStatus::Applied);
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint MUST NOT be called again (surviving local state)"
    );

    // CRASH SEAM REPLAY: Simulate complete loss of local state (state deleted, new machine, state path changed)
    let state_path_wiped = temp.path().join("state_wiped.json");
    let mut state_wiped = BridgeStateStore::load_or_create(&state_path_wiped).unwrap();
    let receipt3 = executor
        .process_discovered_intent(&discovered, &mut state_wiped, None)
        .await;
    assert_eq!(receipt3.status, ReceiptStatus::Applied);
    assert_eq!(
        receipt3
            .lantern
            .as_ref()
            .and_then(|l| l.record_id.as_deref()),
        Some("019323ef-6258-75b2-a42e-13c2f0fcf6d6"),
        "Original Lantern record ID must be preserved even when local state was lost"
    );
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint MUST NOT be called again when local state was lost (canonical idempotency)"
    );

    // Now test tampering: same intent_id, but payload modified!
    let intent_tampered = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999IDEMP0000000001",
        "action": "memory.create",
        "requested_by": { "actor": "client", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "payload": { "content": "Tampered payload!" }
    });
    let discovered_tampered = DiscoveredIntentCommit {
        commit_sha: "commit_idemp_tamper".to_string(),
        intent_file_path: "intents/01K999IDEMP0000000001.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_tampered).unwrap(),
    };

    let receipt_tampered = executor
        .process_discovered_intent(&discovered_tampered, &mut state, None)
        .await;
    assert_eq!(receipt_tampered.status, ReceiptStatus::Suspect);
    assert!(
        receipt_tampered
            .error
            .unwrap()
            .contains("idempotency conflict: payload mismatch")
    );
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint MUST NOT be called for tampered replay"
    );

    // Tampered replay when local state is lost (canonical datastore catches digest mismatch)
    let state_path_wiped2 = temp.path().join("state_wiped2.json");
    let mut state_wiped2 = BridgeStateStore::load_or_create(&state_path_wiped2).unwrap();
    let receipt_tampered2 = executor
        .process_discovered_intent(&discovered_tampered, &mut state_wiped2, None)
        .await;
    assert_eq!(receipt_tampered2.status, ReceiptStatus::Suspect);
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint MUST NOT be called for tampered replay even when local state is lost"
    );
}

#[tokio::test]
async fn test_memory_reinforce_is_explicitly_disabled() {
    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();

    let tethers = TethersGate::new("http://127.0.0.1:9".to_string(), None, None);
    let executor = BridgeExecutor::new("http://127.0.0.1:9".to_string(), tethers, None);

    // target must only have record_id
    let intent = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999REINFORCE00001",
        "action": "memory.reinforce",
        "requested_by": { "actor": "client", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "target": {
            "record_id": "019323ef-6258-75b2-a42e-13c2f0fcf6d6"
        },
        "payload": {
            "delta": 1.0
        }
    });

    let discovered = DiscoveredIntentCommit {
        commit_sha: "commit_reinf_1".to_string(),
        intent_file_path: "intents/01K999REINFORCE00001.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent).unwrap(),
    };

    let receipt = executor
        .process_discovered_intent(&discovered, &mut state, None)
        .await;
    assert_eq!(receipt.status, ReceiptStatus::Denied);
    assert!(
        receipt
            .error
            .unwrap()
            .contains("action_disabled: reinforcement storage seam deferred")
    );
}

#[test]
fn test_status_bridge_json_roundtrip() {
    let status = BridgeStatusFile {
        bridge_version: "0.1.0".to_string(),
        last_cycle_at: Some("2026-10-02T05:00:00Z".to_string()),
        last_success_at: Some("2026-10-02T05:00:00Z".to_string()),
        last_processed_inbox_commit: Some("commit_123".to_string()),
        inbox_head: Some("commit_123".to_string()),
        pending_count: 0,
        last_terminal_error: None,
        lantern_reachable: true,
        tethers_reachable: true,
        authority_path_reachable: true,
        tethers_engine_present: true,
        mutation_ready: true,
        last_receipt_outcome: Some("APPLIED (intent: 123, record: rec_1)".to_string()),
        lantern_version: Some("0.1.0".to_string()),
        datastore_mode: Some("embedded-surrealkv".to_string()),
        datastore_mode_observed: Some("embedded-surrealkv".to_string()),
        surrealdb_expected_version: Some("3.3.0".to_string()),
        surrealdb_observed_version: Some("3.3.0".to_string()),
        post_repo_valid: true,
        mirror_last_generated_at: Some("2026-10-02T05:00:00Z".to_string()),
    };

    let json_str = serde_json::to_string_pretty(&status).unwrap();
    let loaded: BridgeStatusFile = serde_json::from_str(&json_str).unwrap();

    assert_eq!(loaded.bridge_version, "0.1.0");
    assert_eq!(
        loaded.last_processed_inbox_commit.as_deref(),
        Some("commit_123")
    );
    assert_eq!(loaded.pending_count, 0);
    assert!(loaded.lantern_reachable);
    assert!(loaded.tethers_reachable);
    assert!(loaded.authority_path_reachable);
    assert!(loaded.tethers_engine_present);
    assert!(loaded.mutation_ready);
    assert_eq!(loaded.datastore_mode.as_deref(), Some("embedded-surrealkv"));
    assert_eq!(loaded.surrealdb_expected_version.as_deref(), Some("3.3.0"));
    assert_eq!(
        loaded.mirror_last_generated_at.as_deref(),
        Some("2026-10-02T05:00:00Z")
    );
}

#[tokio::test]
async fn test_doctor_unavailable_lighting_reports_unreachable() {
    let temp = TestTempDir::new();
    let post_repo = temp.path().join("lantern-post");
    let state_file = temp.path().join("bridge-state.json");
    std::fs::create_dir_all(&post_repo).unwrap();

    let report =
        super::doctor::DoctorReport::run("http://127.0.0.1:65530", &post_repo, None, &state_file)
            .await;

    assert!(!report.lantern_reachable);
    assert!(!report.authority_path_reachable);
    assert!(!report.tethers_available);
    assert!(!report.mutation_safe_to_enable);
}

#[tokio::test]
async fn test_tethers_executable_alone_does_not_imply_authority_reachable() {
    let temp = TestTempDir::new();
    let dummy_engine = temp.path().join("dummy_tethers.exe");
    std::fs::write(&dummy_engine, b"mock binary").unwrap();

    let gate = super::tethers::TethersGate::new(
        "http://127.0.0.1:65530".to_string(),
        None,
        Some(dummy_engine),
    );

    // Engine executable is present on disk
    assert!(gate.is_engine_available());
    // BUT authority path is unreachable because service is not running
    assert!(!gate.probe_authority_path().await);
}

#[test]
fn test_idle_cycle_writes_status_file_to_disk() {
    let temp = TestTempDir::new();
    let post_repo = temp.path().join("lantern-post");
    let queue = GitQueue::new(&post_repo);

    let status = BridgeStatusFile {
        bridge_version: "0.1.0".to_string(),
        last_cycle_at: Some("2026-10-02T08:00:00Z".to_string()),
        last_success_at: Some("2026-10-02T08:00:00Z".to_string()),
        last_processed_inbox_commit: None,
        inbox_head: None,
        pending_count: 0,
        last_terminal_error: None,
        lantern_reachable: true,
        tethers_reachable: true,
        authority_path_reachable: true,
        tethers_engine_present: false,
        mutation_ready: true,
        last_receipt_outcome: None,
        lantern_version: Some("0.1.0".to_string()),
        datastore_mode: Some("embedded-surrealkv".to_string()),
        datastore_mode_observed: Some("embedded-surrealkv".to_string()),
        surrealdb_expected_version: Some("3.3.0".to_string()),
        surrealdb_observed_version: Some("3.3.0".to_string()),
        post_repo_valid: true,
        mirror_last_generated_at: None,
    };

    queue.write_status_file(&status).unwrap();

    let written_file = post_repo.join("status").join("bridge.json");
    assert!(written_file.exists());
    let content = std::fs::read_to_string(written_file).unwrap();
    let parsed: BridgeStatusFile = serde_json::from_str(&content).unwrap();
    assert_eq!(
        parsed.last_cycle_at.as_deref(),
        Some("2026-10-02T08:00:00Z")
    );
    assert!(parsed.authority_path_reachable);
}

#[tokio::test]
async fn hostile_authority_history() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TestTempDir::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let endpoint_calls = calls.clone();
    let app = Router::new()
        .route(
            "/api/v1/tethers/authority/check",
            post(|Json(value): Json<serde_json::Value>| async move {
                let decision = match value["principal_id"].as_str().unwrap_or("") {
                    "agent:allow" => "ALLOW",
                    "agent:ask" => "ASK",
                    _ => "DENY",
                };
                Json(json!({"decision":decision,"grant_id":"fixture","reason":"fixture decision"}))
            }),
        )
        .route(
            "/api/v1/memories",
            post(move || {
                let calls = endpoint_calls.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Json(json!({"id":"019323ef-6258-75b2-a42e-13c2f0fcf6d6","content":"fixture"}))
                }
            }),
        );
    // Canonical claim/complete are mocked here: this fixture qualifies the real
    // runner's local checkpoint/replay and independent mutation counts, not
    // canonical SurrealKV durability (covered separately by store tests).
    let app = app
        .route(
            "/api/v1/bridge/intents/claim",
            post(|Json(req): Json<serde_json::Value>| async move {
                Json(json!({"outcome": "claimed", "record": {
                    "intent_id": req["intent_id"], "status": "CLAIMED"
                }}))
            }),
        )
        .route(
            "/api/v1/bridge/intents/complete",
            post(|Json(req): Json<serde_json::Value>| async move { Json(json!({"intent": req})) }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let dead = TcpListener::bind("127.0.0.1:0").await?;
    let dead_url = format!("http://{}", dead.local_addr()?);
    drop(dead);
    let mut history = Vec::new();
    for actor in ["allow", "ask", "deny", "unavailable"] {
        let gate = TethersGate::new(
            if actor == "unavailable" {
                dead_url.clone()
            } else {
                url.clone()
            },
            None,
            None,
        );
        let executor = BridgeExecutor::new(url.clone(), gate, None);
        let mut state =
            BridgeStateStore::load_or_create(&temp.path().join(format!("{actor}.json")))?;
        let raw = json!({"schema":"lantern.intent.v1","intent_id":format!("hostile-{actor}"),"action":"memory.create","requested_by":{"actor":actor,"transport":"github"},"created_at":"2026-10-03T00:00:00Z","payload":{"content":"disposable fixture"}});
        let discovered = DiscoveredIntentCommit {
            commit_sha: format!("fixture-{actor}"),
            intent_file_path: format!("intents/hostile-{actor}.json"),
            intent_raw_json: serde_json::to_string(&raw)?,
        };
        let before = calls.load(std::sync::atomic::Ordering::SeqCst);
        let receipt = executor
            .process_discovered_intent(&discovered, &mut state, None)
            .await;
        let after = calls.load(std::sync::atomic::Ordering::SeqCst);
        history.push(
            json!({"actor":actor,"before_calls":before,"after_calls":after,"receipt":receipt}),
        );
    }
    server.abort();
    let _ = server.await;
    println!(
        "TB_BRIDGE_HISTORY={}",
        json!({"schema":"lantern-authority-history/v1","history":history})
    );
    Ok(())
}

// Full offline runner cycles against an owned local Git remote and counted mock endpoint.
#[tokio::test]
async fn hostile_restarted_cycles_history() -> Result<(), Box<dyn std::error::Error>> {
    use super::runner::{BridgeConfig, BridgeRunner};
    let temp = TestTempDir::new();
    let queue_path = temp.path().join("queue");
    std::fs::create_dir_all(&queue_path)?;
    init_test_git_repo(&queue_path);
    run_git_cmd(&queue_path, &["branch", "receipts"]);
    let remote_path = temp
        .path()
        .join("matthewjameswatkins1978-cyber")
        .join("lantern-post.git");
    std::fs::create_dir_all(remote_path.parent().unwrap())?;
    let remote = remote_path.to_string_lossy().replace('\\', "/");
    run_git_cmd(temp.path(), &["init", "--bare", &remote]);
    run_git_cmd(&queue_path, &["remote", "add", "origin", &remote]);
    run_git_cmd(&queue_path, &["push", "origin", "--all"]);
    let intents_path = queue_path.join("intents");
    std::fs::create_dir_all(&intents_path)?;
    let expected = ["hostile-intent-A", "hostile-intent-B", "hostile-intent-C"];
    for id in expected {
        let raw = json!({"schema":"lantern.intent.v1","intent_id":id,"action":"memory.create","requested_by":{"actor":"fixture","transport":"github"},"created_at":"2026-10-03T00:00:00Z","payload":{"content":id}});
        std::fs::write(
            intents_path.join(format!("{id}.json")),
            serde_json::to_string(&raw)?,
        )?;
        run_git_cmd(&queue_path, &["add", "intents"]);
        run_git_cmd(&queue_path, &["commit", "-m", id]);
    }
    run_git_cmd(&queue_path, &["push", "origin", "inbox"]);
    let head = run_git_cmd(&queue_path, &["rev-parse", "inbox"]);
    let mutation_path = temp.path().join("mutations.json");
    std::fs::write(&mutation_path, "[]")?;
    let mutation_file = mutation_path.clone();
    let app = Router::new()
        .route("/api/v1/tethers/authority/check", post(|| async { Json(json!({"decision":"ALLOW","grant_id":"fixture"})) }))
        .route("/api/v1/memories", post(move |Json(value): Json<serde_json::Value>| {
            let path = mutation_file.clone();
            async move {
                use std::io::Write;
                let mut rows: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                rows.push(value.clone());
                let mut file = std::fs::File::create(path).unwrap();
                file.write_all(serde_json::to_string(&rows).unwrap().as_bytes()).unwrap();
                file.sync_all().unwrap();
                Json(json!({"id":"019323ef-6258-75b2-a42e-13c2f0fcf6d6","content":value["content"]}))
            }
        }));
    // Canonical claim/complete are mocked here: this fixture qualifies the real
    // runner's local checkpoint/replay and independent mutation counts, not
    // canonical SurrealKV durability (covered separately by store tests).
    let app = app
        .route(
            "/api/v1/bridge/intents/claim",
            post(|Json(req): Json<serde_json::Value>| async move {
                Json(json!({"outcome": "claimed", "record": {
                    "intent_id": req["intent_id"], "status": "CLAIMED"
                }}))
            }),
        )
        .route(
            "/api/v1/bridge/intents/complete",
            post(|Json(req): Json<serde_json::Value>| async move { Json(json!({"intent": req})) }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let state_path = temp.path().join("state.json");
    let config = BridgeConfig {
        service_url: url,
        post_repo_path: queue_path.clone(),
        git_repo_path: None,
        state_path: state_path.clone(),
        poll_interval_secs: 1,
        push: false,
        audit_token: None,
        tethers_engine_path: None,
    };
    let mut cycles = Vec::new();
    for iteration in 0..3 {
        let runner = BridgeRunner::new(config.clone());
        let before: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&mutation_path)?)?;
        let result = runner.run_once().await;
        drop(runner);
        let reopened = BridgeStateStore::load_or_create(&state_path)?;
        let after: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&mutation_path)?)?;
        cycles.push(json!({"iteration":iteration,"before_calls":before.len(),"after_calls":after.len(),"completed":result.is_ok(),"error":result.err().map(|e|format!("{e:#}")),"reopened_state":reopened.data}));
    }
    server.abort();
    let _ = server.await;
    let mutations: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&mutation_path)?)?;
    let mut receipts = Vec::new();
    for entry in std::fs::read_dir(queue_path.join("receipts"))? {
        receipts.push(serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(entry?.path())?,
        )?);
    }
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(queue_path.join("status/bridge.json"))?)?;
    println!(
        "TB_BRIDGE_HISTORY={}",
        json!({"schema":"lantern-runner-history/v1","cycles":cycles,"expected_intents":expected,"inbox_head":head,"mutations":mutations,"receipts":receipts,"status":status,"remote_kind":"disposable_local_bare_git","push_enabled":false})
    );
    Ok(())
}
