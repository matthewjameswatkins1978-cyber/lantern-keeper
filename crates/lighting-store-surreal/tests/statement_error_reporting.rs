//! Regression tests for statement-level failure reporting (LK-CUTOVER-001 repair).
//!
//! Before this fix `EpistemicRepository` writes issued a SurrealQL statement and
//! returned `Ok` whenever the *driver call* succeeded. A statement rejected
//! inside a `BEGIN … COMMIT` batch rolls the whole transaction back, but the
//! response carried the error only in its per-statement results, so the caller
//! recorded an accepted write that did not exist.
//!
//! These tests fail against the old behaviour and pass against the checked one:
//! every rejected write must return `Err`, and nothing from the rejected batch
//! may persist.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, BeliefRevision, BeliefRevisionId, Claim, EpistemicRepository, NewBelief,
    NewClaim, PredicateStatus, ReconciliationAction, Scope, Stance, Trace, TraceId, TrustClass,
};
use lighting_store_surreal::{StoreConfig, SurrealEpistemicRepository, SurrealStore};
use uuid::Uuid;

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-stmt-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf, database: &str) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "statement_errors".to_owned();
    config.database = database.to_owned();
    config.username.clear();
    config.password.clear();
    config
}

async fn harness(label: &str) -> Result<SurrealEpistemicRepository, Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&embedded_config(test_path(label), "regression")).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    Ok(repository)
}

fn scope() -> Scope {
    BTreeMap::from([("project".to_owned(), "workshop".to_owned())])
}

fn belief(value: &str) -> Belief {
    let at = Utc::now();
    Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "matthew".to_owned(),
        predicate_key: "preferred_editor".to_owned(),
        current_value: value.to_owned(),
        scope: scope(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: at,
        valid_from: Some(at),
        created_at: at,
    })
    .expect("belief fixture")
}

fn claim(value: &str) -> Claim {
    let at = Utc::now();
    Claim::new(NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: "matthew".to_owned(),
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: value.to_owned(),
        scope: scope(),
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: at,
        valid_from: Some(at),
        valid_to: None,
        extractor: "statement-errors".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: at,
    })
    .expect("claim fixture")
}

fn trace() -> Trace {
    Trace {
        id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        event_type: "repair.test".to_owned(),
        actor_id: "matthew".to_owned(),
        subject_id: None,
        input_ids: Vec::new(),
        output_ids: Vec::new(),
        details: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn revision(belief_id: &BeliefId, from: Option<&str>, to: &str) -> BeliefRevision {
    BeliefRevision {
        id: BeliefRevisionId::new(Uuid::new_v4().to_string()).expect("revision id"),
        belief_id: belief_id.clone(),
        previous_value: from.map(str::to_owned),
        new_value: Some(to.to_owned()),
        claim_id: None,
        transition_type: ReconciliationAction::Supersede,
        trace_id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        created_at: Utc::now(),
    }
}

/// A duplicate revision id is a genuine stale/competing-write refusal: the store
/// did accept that transition marker already.
#[tokio::test]
async fn stale_revision_refusal_reports_failure_and_persists_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = harness("stale").await?;
    let first = belief("zed");
    repository.store_belief(first.clone()).await?;

    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = Utc::now();
    let accepted = revision(&first.id, Some("zed"), "helix");
    repository
        .apply_belief_transition(
            Some(first.clone()),
            Some(second.clone()),
            vec![accepted.clone()],
            trace(),
            Vec::new(),
        )
        .await?;

    let mut third = second.clone();
    third.current_value = "vim".to_owned();
    third.updated_at = Utc::now();
    let stale = BeliefRevision {
        id: accepted.id.clone(),
        created_at: Utc::now(),
        ..revision(&first.id, Some("helix"), "vim")
    };

    let refused = repository
        .apply_belief_transition(
            Some(second.clone()),
            Some(third),
            vec![stale],
            trace(),
            Vec::new(),
        )
        .await;
    assert!(
        refused.is_err(),
        "a rejected batch must report failure, got {refused:?}"
    );

    assert_eq!(
        repository.get_belief(&first.id).await?,
        Some(second),
        "a rejected transition must not move the projection"
    );
    let revisions = repository.list_belief_revisions(&first.id).await?;
    assert_eq!(
        revisions.len(),
        1,
        "a rejected transition must leave exactly the accepted revision, found {revisions:?}"
    );
    assert_eq!(revisions[0].id, accepted.id);
    Ok(())
}

/// A transition whose target id already exists cannot create the projection; the
/// batch must report that rather than claim success.
#[tokio::test]
async fn forged_projection_create_reports_failure() -> Result<(), Box<dyn std::error::Error>> {
    let repository = harness("forged").await?;
    let existing = belief("zed");
    repository.store_belief(existing.clone()).await?;

    // previous = None forces a CREATE of a belief id that already exists.
    let mut clone = existing.clone();
    clone.current_value = "vim".to_owned();
    clone.updated_at = Utc::now();
    let refused = repository
        .apply_belief_transition(
            None,
            Some(clone),
            vec![revision(&existing.id, None, "vim")],
            trace(),
            Vec::new(),
        )
        .await;
    assert!(
        refused.is_err(),
        "a create that duplicates a projection id must report failure, got {refused:?}"
    );
    assert_eq!(
        repository.list_belief_revisions(&existing.id).await?.len(),
        0,
        "the rejected batch must not leave an orphan revision"
    );
    assert_eq!(
        repository.get_belief(&existing.id).await?,
        Some(existing),
        "the rejected batch must not modify the existing projection"
    );
    Ok(())
}

/// A multi-statement batch where the *last* statement is unacceptable must abort
/// everything, including the earlier statements in the same batch.
#[tokio::test]
async fn partial_batch_aborts_and_reports_failure() -> Result<(), Box<dyn std::error::Error>> {
    let repository = harness("partial").await?;
    let first = belief("zed");
    repository.store_belief(first.clone()).await?;

    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = Utc::now();

    // A dependent belief that does not exist: the stale UPDATE affects no rows,
    // and the duplicate revision id then rejects the batch.
    let mut ghost = belief("ghost");
    ghost.mark_stale("dependency moved", Utc::now());
    let duplicate = {
        let existing = revision(&first.id, Some("zed"), "helix");
        repository
            .apply_belief_transition(
                Some(first.clone()),
                Some(second.clone()),
                vec![existing.clone()],
                trace(),
                Vec::new(),
            )
            .await?;
        existing
    };

    let mut attempted = second.clone();
    attempted.current_value = "vim".to_owned();
    attempted.updated_at = Utc::now();
    let refused = repository
        .apply_belief_transition(
            Some(second.clone()),
            Some(attempted),
            vec![BeliefRevision {
                id: duplicate.id.clone(),
                created_at: Utc::now(),
                ..revision(&first.id, Some("helix"), "vim")
            }],
            trace(),
            vec![ghost.clone()],
        )
        .await;
    assert!(
        refused.is_err(),
        "a batch rejected mid-way must report failure, got {refused:?}"
    );
    assert!(
        repository.get_belief(&ghost.id).await?.is_none(),
        "the rejected batch must not create or stale an unrelated projection"
    );
    Ok(())
}

/// Single-statement writers had the same defect: a rejected CREATE reported `Ok`.
#[tokio::test]
async fn duplicate_single_statement_writes_report_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = harness("single").await?;
    let belief = belief("zed");
    repository.store_belief(belief.clone()).await?;

    let duplicate_belief = repository.store_belief(belief.clone()).await;
    assert!(
        duplicate_belief.is_err(),
        "storing the same belief id twice must report failure, got {duplicate_belief:?}"
    );

    // Claims dedupe on content, so an identical replay legitimately returns the
    // stored record. To exercise the *rejected insert* path the id must repeat
    // while the dedupe key differs.
    let claim = claim("zed");
    repository.store_claim(claim.clone()).await?;
    let mut collided = claim;
    collided.value = "different value".to_owned();
    collided.dedupe_key = "different|dedupe|key|value".to_owned();
    let duplicate_claim = repository.store_claim(collided).await;
    assert!(
        duplicate_claim.is_err(),
        "storing the same claim id twice must report failure, got {duplicate_claim:?}"
    );
    Ok(())
}

/// The success path must be untouched: an accepted transition stays `Ok`, moves
/// the projection and survives a reopen.
#[tokio::test]
async fn accepted_transition_stays_successful_and_durable() -> Result<(), Box<dyn std::error::Error>>
{
    let path = test_path("durable");
    let repository = {
        let store = SurrealStore::connect(&embedded_config(path.clone(), "durability")).await?;
        store.initialise_schema().await?;
        let repository = SurrealEpistemicRepository::new(store);
        repository.migrate().await?;
        repository
    };

    let first = belief("zed");
    repository.store_belief(first.clone()).await?;
    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = Utc::now();
    let receipt = repository
        .apply_belief_transition(
            Some(first.clone()),
            Some(second.clone()),
            vec![revision(&first.id, Some("zed"), "helix")],
            trace(),
            Vec::new(),
        )
        .await;
    assert!(
        receipt.is_ok(),
        "an accepted transition must still report success, got {receipt:?}"
    );
    assert_eq!(repository.get_belief(&first.id).await?, Some(second));

    // SurrealKV holds an exclusive directory lock: drop the handle before reopening.
    drop(repository);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let reopened = SurrealStore::connect(&embedded_config(path, "durability")).await?;
    let after_restart = SurrealEpistemicRepository::new(reopened);
    let durable = after_restart
        .get_belief(&first.id)
        .await?
        .expect("belief must survive reopen");
    assert_eq!(durable.current_value, "helix");
    assert_eq!(
        after_restart.list_belief_revisions(&first.id).await?.len(),
        1
    );
    Ok(())
}
