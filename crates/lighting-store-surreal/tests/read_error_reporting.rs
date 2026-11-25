//! Regression tests for statement-level failures on *read* paths (LK-Q01).
//!
//! `Query::take(0)` reports `Ok` with zero rows when the statement was rejected:
//! measured on surrealdb 4.0.1 / surrealkv 1.4.0, a query whose statement errored
//! (`take_errors` non-empty) still returned `Ok(0 rows)`. Every single-row lookup
//! is built on that call, so a failed query was indistinguishable from "this
//! record does not exist" — the wrong answer for an existence check.
//!
//! These tests pin the distinction: an absent record stays `Ok(None)`, a failed
//! query is `Err`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, EpistemicRepository, NewBelief, NewClaim, PredicateStatus, Scope, Stance,
    TrustClass,
};
use lighting_store_surreal::{StoreConfig, SurrealEpistemicRepository, SurrealStore};
use uuid::Uuid;

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-readerr-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "read_errors".to_owned();
    config.database = "regression".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

struct Harness {
    repository: SurrealEpistemicRepository,
    store: SurrealStore,
}

async fn harness(label: &str) -> Result<Harness, Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&embedded_config(test_path(label))).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store.clone());
    repository.migrate().await?;
    Ok(Harness { repository, store })
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

/// An absent record must stay an absence: the fix must not turn "not found" into
/// an error.
#[tokio::test]
async fn absent_records_are_still_reported_as_absent() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("absent").await?;
    let missing = BeliefId::new(Uuid::new_v4().to_string())?;
    assert_eq!(
        harness.repository.get_belief(&missing).await?,
        None,
        "an untouched table with an unknown id must report absence, not failure"
    );

    let claim = NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: "matthew".to_owned(),
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: "value".to_owned(),
        scope: scope(),
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: Utc::now(),
        valid_from: None,
        valid_to: None,
        extractor: "read-errors".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: Utc::now(),
    };
    let claim = lighting_core::Claim::new(claim)?;
    assert_eq!(
        harness.repository.get_claim(&claim.id).await?,
        None,
        "an unknown claim id must report absence, not failure"
    );
    Ok(())
}

/// A rejected SELECT must be an error, not an empty result.
///
/// Injection: removing the table makes the statement fail on this engine
/// (measured: `take_errors` reports "table does not exist" while `take(0)`
/// returns `Ok(0 rows)`). The assertion is deliberately about the *distinction*,
/// not the message.
#[tokio::test]
async fn failed_read_reports_failure_not_absence() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("failure").await?;
    let stored = belief("zed");
    harness.repository.store_belief(stored.clone()).await?;
    assert_eq!(
        harness.repository.get_belief(&stored.id).await?,
        Some(stored.clone()),
        "the record must be readable before the injection"
    );

    harness.store.query("REMOVE TABLE belief").await?;

    let outcome = harness.repository.get_belief(&stored.id).await;
    assert!(
        outcome.is_err(),
        "a rejected SELECT must surface as an error, not as Ok(None); got {outcome:?}"
    );

    let absent = BeliefId::new(Uuid::new_v4().to_string())?;
    let absent_outcome = harness.repository.get_belief(&absent).await;
    assert!(
        absent_outcome.is_err(),
        "after the injection even an unknown id cannot be answered with confidence"
    );
    Ok(())
}

/// The dedupe lookup that backs claim storage uses the same helper, so a failed
/// lookup must not be mistaken for "no existing claim" (which would attempt a
/// duplicate insert on a broken table).
#[tokio::test]
async fn failed_dedupe_lookup_reports_failure() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("dedupe").await?;
    let claim = lighting_core::Claim::new(NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: "matthew".to_owned(),
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: "zed".to_owned(),
        scope: scope(),
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: Utc::now(),
        valid_from: None,
        valid_to: None,
        extractor: "read-errors".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: Utc::now(),
    })?;
    harness.repository.store_claim(claim.clone()).await?;

    harness.store.query("REMOVE TABLE epistemic_claim").await?;

    let replay = harness.repository.store_claim(claim).await;
    assert!(
        replay.is_err(),
        "a failed dedupe lookup must not be treated as 'no existing claim'; got {replay:?}"
    );
    Ok(())
}
