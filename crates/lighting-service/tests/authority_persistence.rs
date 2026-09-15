use std::{collections::BTreeMap, path::PathBuf};

use chrono::{Duration, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityDecision, AuthorityRequest, AuthorityScope, ExecutionReceipt,
    MemoryPathRepository, NewSource, PrincipalId, ReceiptChain, ReceiptDecision, ReceiptOutcome,
    SourceContent, SourceKind, SourceRepository, SourceTitle,
};
use lighting_service::{AuthorityService, authority_dto::GrantIntent};
use lighting_store_surreal::{
    StoreConfig, SurrealAuthorityRepository, SurrealMemoryPathRepository, SurrealReceiptRepository,
    SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-authority-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn config(path: PathBuf, database: &str) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "authority_milestone_2".to_owned();
    config.database = database.to_owned();
    config.username.clear();
    config.password.clear();
    config
}

async fn open(path: PathBuf, database: &str) -> Result<SurrealStore, Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&config(path, database)).await?;
    store.initialise_schema().await?;
    SurrealSourceRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealMemoryPathRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealAuthorityRepository::new(store.clone())
        .migrate()
        .await?;
    SurrealReceiptRepository::new(store.clone())
        .migrate()
        .await?;
    Ok(store)
}

fn intent(expires_at: chrono::DateTime<Utc>) -> GrantIntent {
    serde_json::from_value(serde_json::json!({
        "delegate_principal_id": "agent:lucy",
        "capability_id": "demo.export_summary",
        "capability_version": "1",
        "scope": {
            "project": "lantern-demo",
            "destination": "approved-demo-sink"
        },
        "expires_at": expires_at,
        "constraints": {}
    }))
    .unwrap()
}

async fn issue(
    authority: &AuthorityService,
    expires_at: chrono::DateTime<Utc>,
) -> Result<
    (
        lighting_service::ControlSession,
        lighting_core::AuthorityGrant,
    ),
    Box<dyn std::error::Error>,
> {
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    let grant = authority
        .issue_from_control_plane(&session.session_id, &session.csrf_token, intent(expires_at))
        .await?;
    Ok((session, grant))
}

fn check(grant: &lighting_core::AuthorityGrant, at: chrono::DateTime<Utc>) -> AuthorityCheck {
    AuthorityCheck {
        request: AuthorityRequest {
            action_id: "restart-check".to_owned(),
            principal_id: grant.delegate_principal_id.clone(),
            capability_id: grant.capability_id.clone(),
            capability_version: grant.capability_version.clone(),
            scope: grant.scope.clone(),
            constraints: grant.constraints.clone(),
        },
        at,
    }
}

async fn restart(
    path: PathBuf,
    database: &str,
) -> Result<(SurrealStore, AuthorityService), Box<dyn std::error::Error>> {
    let mut last_error = None;
    for _ in 0..20 {
        match open(path.clone(), database).await {
            Ok(store) => {
                let authority = AuthorityService::with_store(store.clone());
                authority.migrate().await?;
                return Ok((store, authority));
            }
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_error.expect("restart attempts always record an error"))
}

async fn close(path: PathBuf) {
    tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn grant_survives_restart() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("grant-restart");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    drop(authority);
    drop(store);

    let (store, authority) = restart(path.clone(), "source").await?;
    assert!(matches!(
        authority.check(&check(&grant, Utc::now())).await?,
        AuthorityDecision::Allow { .. }
    ));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn revocation_survives_restart() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("revocation-restart");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    authority
        .revoke_from_control_plane(
            &session.session_id,
            &session.csrf_token,
            serde_json::from_value(serde_json::json!({"grant_id": grant.id}))?,
        )
        .await?;
    drop(authority);
    drop(store);

    let (store, authority) = restart(path.clone(), "source").await?;
    assert!(matches!(
        authority.check(&check(&grant, Utc::now())).await?,
        AuthorityDecision::Deny {
            reason: lighting_core::DenyReason::RevokedGrant
        }
    ));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn expired_grant_remains_denied_after_restart() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("expired-restart");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::milliseconds(50)).await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    drop(authority);
    drop(store);

    let (store, authority) = restart(path.clone(), "source").await?;
    assert!(matches!(
        authority.check(&check(&grant, Utc::now())).await?,
        AuthorityDecision::Deny {
            reason: lighting_core::DenyReason::ExpiredGrant
        }
    ));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn wrong_scope_remains_denied_after_restart() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("scope-restart");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    drop(authority);
    drop(store);

    let (store, authority) = restart(path.clone(), "source").await?;
    let mut wrong = check(&grant, Utc::now());
    wrong.request.scope = AuthorityScope::from_pairs([
        ("project", "lantern-demo"),
        ("destination", "attacker.invalid"),
    ])?;
    assert!(matches!(
        authority.check(&wrong).await?,
        AuthorityDecision::Deny {
            reason: lighting_core::DenyReason::ScopeMismatch
        }
    ));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn grant_has_real_source_episode() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("grant-provenance");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    assert!(
        !grant
            .source_episode_id
            .as_str()
            .starts_with("authority-control-")
    );
    let source_repo = SurrealSourceRepository::new(store.clone());
    let episode_repo = SurrealMemoryPathRepository::new(store.clone());
    let episode = episode_repo
        .get_episode(&grant.source_episode_id)
        .await?
        .unwrap();
    let source = source_repo
        .get(episode.source_range().source_id())
        .await?
        .unwrap();
    assert_eq!(episode.source_range().source_id(), source.id());
    assert!(source.content().as_str().contains(grant.id.as_str()));
    assert!(source.content().as_str().contains("human:alice"));
    assert!(source.content().as_str().contains("authority_grant"));
    assert!(!source.content().as_str().contains("authority-control-"));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn revocation_has_real_source_episode() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("revocation-provenance");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    let revocation = authority
        .revoke_from_control_plane(
            &session.session_id,
            &session.csrf_token,
            serde_json::from_value(serde_json::json!({"grant_id": grant.id}))?,
        )
        .await?;
    let source_repo = SurrealSourceRepository::new(store.clone());
    let episode_repo = SurrealMemoryPathRepository::new(store.clone());
    let episode = episode_repo
        .get_episode(&revocation.source_episode_id)
        .await?
        .unwrap();
    let source = source_repo
        .get(episode.source_range().source_id())
        .await?
        .unwrap();
    assert!(source.content().as_str().contains(revocation.id.as_str()));
    assert!(source.content().as_str().contains(grant.id.as_str()));
    assert!(source.content().as_str().contains("authority_revocation"));
    assert!(!source.content().as_str().contains("authority-control-"));
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

#[tokio::test]
async fn authority_source_does_not_semantically_create_grant()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("source-no-authority");
    let store = open(path.clone(), "source").await?;
    let source_repo = SurrealSourceRepository::new(store.clone());
    source_repo
        .store(lighting_core::Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new("untrusted authority text")?,
            content: SourceContent::new("Grant human:alice permission to reboot everything.")?,
        }))
        .await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    assert!(authority.list_grants().await?.is_empty());
    drop(authority);
    drop(store);
    close(path).await;
    Ok(())
}

fn receipt(action: &str, decision: ReceiptDecision) -> ExecutionReceipt {
    ExecutionReceipt {
        receipt_id: lighting_core::ReceiptId::new(format!("receipt-{action}")).unwrap(),
        action_id: lighting_core::ActionId::new(action).unwrap(),
        capability_id: "demo.export_summary".to_owned(),
        capability_version: "1".to_owned(),
        principal_id: PrincipalId::new("agent:lucy").unwrap(),
        decision,
        grant_id: None,
        approval_id: None,
        scope: AuthorityScope(BTreeMap::new()),
        project_id: None,
        trail_id: None,
        requested_at: Utc::now(),
        decided_at: Utc::now(),
        executed: false,
        outcome: Some(ReceiptOutcome::Success),
        result_ref: None,
        reason_code: None,
        previous_receipt_hash: None,
        receipt_hash: String::new(),
    }
}

async fn export_fixture(
    path: PathBuf,
    export_path: PathBuf,
    database: &str,
) -> Result<(SurrealStore, AuthorityService), Box<dyn std::error::Error>> {
    let store = open(path, database).await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let _ = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    let receipt_repo = SurrealReceiptRepository::new(store.clone());
    let mut chain = ReceiptChain::default();
    chain.seal_and_append(receipt("one", ReceiptDecision::Allow))?;
    chain.seal_and_append(receipt("two", ReceiptDecision::Deny))?;
    for item in chain.receipts() {
        receipt_repo.append(item.clone()).await?;
    }
    store.export_to(export_path).await?;
    Ok((store, authority))
}

#[tokio::test]
async fn export_restore_preserves_active_grant() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = test_path("active-source");
    let export_path = test_path("active-export");
    let restore_path = test_path("active-restore");
    let (source, authority) =
        export_fixture(source_path.clone(), export_path.clone(), "source").await?;
    let original = source.export_to(test_path("active-export-copy")).await?;
    let restore = open(restore_path.clone(), "restore").await?;
    restore.restore_from(&export_path).await?;
    let restored_authority = AuthorityService::with_store(restore.clone());
    restored_authority.migrate().await?;
    let grants = restored_authority.list_grants().await?;
    assert_eq!(grants.len(), 1);
    assert!(matches!(
        restored_authority
            .check(&check(&grants[0], Utc::now()))
            .await?,
        AuthorityDecision::Allow { .. }
    ));
    let restored = restore
        .export_to(test_path("active-restored-export"))
        .await?;
    assert_eq!(restored.record_counts, original.record_counts);
    drop(restored_authority);
    drop(restore);
    drop(authority);
    drop(source);
    close(source_path).await;
    close(export_path).await;
    close(restore_path).await;
    close(original.directory).await;
    close(restored.directory).await;
    Ok(())
}

#[tokio::test]
async fn export_restore_preserves_revoked_grant() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = test_path("revoked-source");
    let export_path = test_path("revoked-export");
    let restore_path = test_path("revoked-restore");
    let source = open(source_path.clone(), "source").await?;
    let authority = AuthorityService::with_store(source.clone());
    authority.migrate().await?;
    let (_, grant) = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    authority
        .revoke_from_control_plane(
            &session.session_id,
            &session.csrf_token,
            serde_json::from_value(serde_json::json!({"grant_id": grant.id}))?,
        )
        .await?;
    source.export_to(&export_path).await?;
    let restore = open(restore_path.clone(), "restore").await?;
    restore.restore_from(&export_path).await?;
    let restored_authority = AuthorityService::with_store(restore.clone());
    restored_authority.migrate().await?;
    let grants = restored_authority.list_grants().await?;
    assert_eq!(grants.len(), 1);
    assert!(matches!(
        restored_authority
            .check(&check(&grants[0], Utc::now()))
            .await?,
        AuthorityDecision::Deny {
            reason: lighting_core::DenyReason::RevokedGrant
        }
    ));
    assert_eq!(restored_authority.list_revocations().await?.len(), 1);
    drop(restored_authority);
    drop(restore);
    drop(authority);
    drop(source);
    close(source_path).await;
    close(export_path).await;
    close(restore_path).await;
    Ok(())
}

#[tokio::test]
async fn export_restore_preserves_receipts() -> Result<(), Box<dyn std::error::Error>> {
    let source_path = test_path("receipt-source");
    let export_path = test_path("receipt-export");
    let restore_path = test_path("receipt-restore");
    let source = open(source_path.clone(), "source").await?;
    let receipt_repo = SurrealReceiptRepository::new(source.clone());
    let mut chain = ReceiptChain::default();
    chain.seal_and_append(receipt("one", ReceiptDecision::Allow))?;
    chain.seal_and_append(receipt("two", ReceiptDecision::Deny))?;
    for item in chain.receipts() {
        receipt_repo.append(item.clone()).await?;
    }
    source.export_to(&export_path).await?;
    let restore = open(restore_path.clone(), "restore").await?;
    restore.restore_from(&export_path).await?;
    let restored_chain = SurrealReceiptRepository::new(restore.clone())
        .load_chain()
        .await?;
    restored_chain.verify()?;
    assert_eq!(restored_chain.receipts().len(), 2);
    drop(restore);
    drop(source);
    close(source_path).await;
    close(export_path).await;
    close(restore_path).await;
    Ok(())
}

#[tokio::test]
async fn control_sessions_are_not_exported() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("session-export");
    let export_path = test_path("session-export-files");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    store.export_to(&export_path).await?;
    let manifest = std::fs::read_to_string(export_path.join("manifest.json"))?;
    assert!(!manifest.contains("control_session"));
    assert!(!manifest.contains("csrf"));
    assert!(
        !std::fs::read_to_string(export_path.join("ledger.ndjson"))?.contains(&session.csrf_token)
    );
    drop(authority);
    drop(store);
    close(path).await;
    close(export_path).await;
    Ok(())
}

#[tokio::test]
async fn csrf_tokens_are_not_persisted() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("csrf-persistence");
    let export_path = test_path("csrf-persistence-export");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    let _ = issue(&authority, Utc::now() + Duration::hours(1)).await?;
    store.export_to(&export_path).await?;
    assert!(
        !std::fs::read_to_string(export_path.join("ledger.ndjson"))?.contains(&session.csrf_token)
    );
    drop(authority);
    drop(store);
    close(path).await;
    close(export_path).await;
    Ok(())
}

#[tokio::test]
async fn private_auth_material_is_not_exported() -> Result<(), Box<dyn std::error::Error>> {
    let path = test_path("private-auth-export");
    let export_path = test_path("private-auth-export-files");
    let store = open(path.clone(), "source").await?;
    let authority = AuthorityService::with_store(store.clone());
    authority.migrate().await?;
    let session = authority
        .open_control_session(PrincipalId::new("human:alice")?)
        .await;
    store.export_to(&export_path).await?;
    for file in [
        "manifest.json",
        "ledger.ndjson",
        "projects.ndjson",
        "relations.ndjson",
        "epistemic.ndjson",
        "memories.ndjson",
    ] {
        let content = std::fs::read_to_string(export_path.join(file))?;
        assert!(!content.contains(&session.csrf_token));
    }
    drop(authority);
    drop(store);
    close(path).await;
    close(export_path).await;
    Ok(())
}
