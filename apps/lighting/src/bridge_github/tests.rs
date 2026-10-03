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
        .process_discovered_intent(&discovered_allow, &mut state)
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
        .process_discovered_intent(&discovered_ask, &mut state)
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
        .process_discovered_intent(&discovered_deny, &mut state)
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
        .process_discovered_intent(&discovered_unavail, &mut state)
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
    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();

    // Start mock server returning recall with a memory that has a known content hash
    let app = Router::new()
        .route(
            "/api/v1/tethers/authority/check",
            post(|| async {
                Json(json!({
                    "decision": "ALLOW",
                    "grant_id": "grant_ok"
                }))
            }),
        )
        .route(
            "/api/v1/memories/recall",
            post(|| async {
                Json(json!({
                    "memories": [{
                        "id": "019323ef-6258-75b2-a42e-13c2f0fcf6d6",
                        "content": "Original live content",
                        "kind": "fact",
                        "status": "active"
                    }]
                }))
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tethers = TethersGate::new(server_url.clone(), None, None);
    let executor = BridgeExecutor::new(server_url, tethers, None);

    // Intent specifies an outdated observed hash (target has record_id only)
    let intent_conflict = json!({
        "schema": "lantern.intent.v1",
        "intent_id": "01K999CONFLICT1234567",
        "action": "memory.archive",
        "requested_by": { "actor": "client", "transport": "github" },
        "created_at": "2026-10-02T05:00:00Z",
        "target": {
            "record_id": "019323ef-6258-75b2-a42e-13c2f0fcf6d6"
        },
        "observed": {
            "record_sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        },
        "payload": {
            "reason": "Archiving outdated memory"
        }
    });

    let discovered = DiscoveredIntentCommit {
        commit_sha: "commit_conflict_1".to_string(),
        intent_file_path: "intents/01K999CONFLICT1234567.json".to_string(),
        intent_raw_json: serde_json::to_string(&intent_conflict).unwrap(),
    };

    let receipt = executor
        .process_discovered_intent(&discovered, &mut state)
        .await;
    assert_eq!(receipt.status, ReceiptStatus::Conflict);
    let err_str = receipt.error.unwrap();
    assert!(err_str.contains("record hash mismatch"));
}

#[tokio::test]
async fn test_executor_idempotency_crash_seam_and_tamper() {
    let temp = TestTempDir::new();
    let state_path = temp.path().join("state.json");
    let mut state = BridgeStateStore::load_or_create(&state_path).unwrap();

    let server_called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_called_clone = Arc::clone(&server_called);

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

    // First execution: mutates Lantern
    let receipt1 = executor
        .process_discovered_intent(&discovered, &mut state)
        .await;
    assert_eq!(receipt1.status, ReceiptStatus::Applied);
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint must be called once"
    );

    // Simulate crash after mutation before receipt was pushed.
    // On reboot/replay of the same intent:
    let receipt2 = executor
        .process_discovered_intent(&discovered, &mut state)
        .await;
    assert_eq!(receipt2.status, ReceiptStatus::Applied);
    assert_eq!(
        server_called.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "Lantern mutation endpoint MUST NOT be called again (crash seam idempotency)"
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
        .process_discovered_intent(&discovered_tampered, &mut state)
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
        .process_discovered_intent(&discovered, &mut state)
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
    assert_eq!(
        loaded.mirror_last_generated_at.as_deref(),
        Some("2026-10-02T05:00:00Z")
    );
}

// Test-only factual export for Terror Bat. Product invariants are checked outside this harness.
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
            .process_discovered_intent(&discovered, &mut state)
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
