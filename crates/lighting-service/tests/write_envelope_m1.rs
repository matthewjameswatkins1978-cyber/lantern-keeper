//! Keyed write-envelope contract for LK-HOST-001 M1 unification.
//!
//! Two things are proven here, and they are deliberately different:
//!
//! 1. the envelope contract an adapter must honour (key binding, version
//!    refusal, revision precondition before the first write, typed refusals);
//! 2. the **native staging risk, retained and recorded**: the native adapter
//!    commits Source, Episode, Claim and projection separately, so a refusal in
//!    the middle leaves earlier stages accepted with no receipt. That is
//!    asserted as current behaviour, not fixed here — the D1 adapter must close
//!    it in one boundary (docs/hosted-m1-d1-atomicity.md).

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use lighting_core::{
    Belief, BeliefRevision, BeliefRevisionId, Claim, CorrectionIdempotencyRecord, Episode,
    EpisodeTitle, EpistemicRepository, MemoryPathRepository, NativeWriteEnvelope, NewBelief,
    NewClaim, NewSource, PredicateStatus, ReconciliationAction, Scope, Source, SourceContent,
    SourceKind, SourceRange, SourceRepository, SourceTitle, Stance, Trace, TraceId, TrustClass,
    WriteEnvelope, WriteEnvelopeError, WriteEnvelopeOutcome, write_envelope_key,
};
use lighting_store_surreal::{
    NATIVE_PARTIAL_COMMIT_RISK, NativeStagedEnvelopeWriter, StoreConfig,
    SurrealEpistemicRepository, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

const NAMESPACE: &str = "m1_write_envelope";

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = NAMESPACE.to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

struct Harness {
    writer: NativeStagedEnvelopeWriter,
    epistemic: SurrealEpistemicRepository,
    sources: SurrealSourceRepository,
    memory_path: SurrealMemoryPathRepository,
}

async fn harness(label: &str) -> Result<Harness, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "lantern-keeper-envelope-{label}-{}",
        Uuid::new_v4().simple()
    ));
    let store = SurrealStore::connect(&embedded_config(path)).await?;
    store.initialise_schema().await?;
    let writer = NativeStagedEnvelopeWriter::new(store.clone());
    writer.migrate().await?;
    Ok(Harness {
        writer,
        epistemic: SurrealEpistemicRepository::new(store.clone()),
        sources: SurrealSourceRepository::new(store.clone()),
        memory_path: SurrealMemoryPathRepository::new(store),
    })
}

fn scope() -> Scope {
    BTreeMap::from([("project".to_owned(), "workshop".to_owned())])
}

fn source(text: &str) -> Source {
    Source::create(NewSource {
        kind: SourceKind::PlainText,
        title: SourceTitle::new(format!("envelope-{text}")).expect("title"),
        content: SourceContent::new(text.to_owned()).expect("content"),
    })
}

fn episode(source: &Source) -> Episode {
    let range = SourceRange::new(
        source.id().clone(),
        0,
        source.content().as_bytes().len(),
        source.content(),
    )
    .expect("range");
    Episode::new(
        EpisodeTitle::new("envelope episode".to_owned()).expect("episode title"),
        range,
    )
}

fn claim(value: &str, at: DateTime<Utc>) -> Claim {
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
        extractor: "m1-envelope".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: at,
    })
    .expect("claim fixture")
}

fn belief(value: &str, at: DateTime<Utc>) -> Belief {
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

fn trace() -> Trace {
    Trace {
        id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        event_type: "envelope.test".to_owned(),
        actor_id: "matthew".to_owned(),
        subject_id: None,
        input_ids: Vec::new(),
        output_ids: Vec::new(),
        details: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn supersede_revision(previous: &Belief, next: &Belief) -> BeliefRevision {
    BeliefRevision {
        id: BeliefRevisionId::new(Uuid::new_v4().to_string()).expect("revision id"),
        belief_id: previous.id.clone(),
        previous_value: Some(previous.current_value.clone()),
        new_value: Some(next.current_value.clone()),
        claim_id: None,
        transition_type: ReconciliationAction::Supersede,
        trace_id: trace().id,
        created_at: Utc::now(),
    }
}

// Test-fixture builder: positional stages read more clearly than a seven-field
// literal repeated in every case, so the arity lint is suppressed here only.
#[allow(clippy::too_many_arguments)]
fn envelope_with(
    caller_key: &str,
    source: Option<Source>,
    episode: Option<Episode>,
    claim: Option<Claim>,
    previous: Option<Belief>,
    current: Option<Belief>,
    revisions: Vec<BeliefRevision>,
    expected_revision: Option<String>,
) -> WriteEnvelope {
    WriteEnvelope {
        principal_id: "matthew".to_owned(),
        scope: scope(),
        caller_key: caller_key.to_owned(),
        expected_revision,
        source,
        episode,
        claim,
        previous,
        current,
        revisions,
        trace: trace(),
        stale_beliefs: Vec::new(),
    }
}

#[tokio::test]
async fn envelope_accepts_every_present_stage_with_one_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("accept").await?;
    let at = Utc::now();
    let source = source("exact bytes");
    let episode = episode(&source);
    let claim = claim("zed", at);
    let projection = belief("zed", at);

    let envelope = envelope_with(
        "capture-accept",
        Some(source.clone()),
        Some(episode.clone()),
        Some(claim.clone()),
        None,
        Some(projection.clone()),
        vec![BeliefRevision {
            id: BeliefRevisionId::new(Uuid::new_v4().to_string())?,
            belief_id: projection.id.clone(),
            previous_value: None,
            new_value: Some("zed".to_owned()),
            claim_id: Some(claim.id.clone()),
            transition_type: ReconciliationAction::Create,
            trace_id: TraceId::new(Uuid::new_v4().to_string())?,
            created_at: at,
        }],
        None,
    );

    let receipt = harness
        .writer
        .apply_write_envelope(envelope.clone())
        .await?;
    assert_eq!(
        receipt.outcome,
        WriteEnvelopeOutcome::Accepted { staged: 4 },
        "source, episode, claim and projection must all be accepted"
    );
    assert_eq!(receipt.version, lighting_core::WRITE_ENVELOPE_VERSION);

    assert!(
        harness.sources.get(source.id()).await?.is_some(),
        "the Source must be readable after acceptance"
    );
    assert!(
        harness
            .memory_path
            .get_episode(episode.id())
            .await?
            .is_some(),
        "the Episode must be readable after acceptance"
    );
    assert!(
        harness.epistemic.get_claim(&claim.id).await?.is_some(),
        "the Claim must be readable after acceptance"
    );
    assert_eq!(
        harness.epistemic.get_belief(&projection.id).await?,
        Some(projection),
        "the projection must be the accepted belief"
    );
    Ok(())
}

#[tokio::test]
async fn envelope_replay_reuses_the_receipt_without_writing_again()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("replay").await?;
    let at = Utc::now();
    let source = source("replay bytes");
    let claim = claim("zed", at);
    let envelope = envelope_with(
        "capture-replay",
        Some(source.clone()),
        None,
        Some(claim.clone()),
        None,
        None,
        Vec::new(),
        None,
    );

    let first = harness
        .writer
        .apply_write_envelope(envelope.clone())
        .await?;
    assert!(matches!(
        first.outcome,
        WriteEnvelopeOutcome::Accepted { .. }
    ));
    let replayed = harness.writer.apply_write_envelope(envelope).await?;
    assert_eq!(
        replayed.outcome,
        WriteEnvelopeOutcome::Replayed,
        "an exact retry must replay the original receipt"
    );
    assert_eq!(replayed.payload_hash, first.payload_hash);
    assert_eq!(
        harness.epistemic.list_claims(false).await?.len(),
        1,
        "a replay must not duplicate the Claim"
    );

    let key = write_envelope_key("matthew", &scope(), "capture-replay");
    let stored = harness
        .epistemic
        .get_correction_idempotency(&key)
        .await?
        .expect("a receipt must exist for the accepted boundary");
    assert_eq!(stored.key, key);
    assert_eq!(stored.payload_hash, first.payload_hash);
    Ok(())
}

#[tokio::test]
async fn envelope_payload_change_conflicts_and_version_change_is_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("conflict").await?;
    let at = Utc::now();
    let source = source("conflict bytes");
    let envelope = envelope_with(
        "capture-conflict",
        Some(source.clone()),
        None,
        Some(claim("zed", at)),
        None,
        None,
        Vec::new(),
        None,
    );
    harness
        .writer
        .apply_write_envelope(envelope.clone())
        .await?;

    let mut changed = envelope.clone();
    changed.claim = Some(claim("helix", at));
    let refused = harness.writer.apply_write_envelope(changed).await;
    assert!(
        matches!(refused, Err(WriteEnvelopeError::PayloadConflict { .. })),
        "the same key with a changed payload must conflict, got {refused:?}"
    );

    // A receipt written by an older envelope version must be refused, not replayed.
    let stale_key = write_envelope_key("matthew", &scope(), "capture-stale-version");
    harness
        .epistemic
        .store_correction_idempotency(CorrectionIdempotencyRecord {
            key: stale_key,
            payload_hash: "0".repeat(64),
            result_json: serde_json::json!({
                "envelope_version": "lantern-keeper-write-envelope-v0",
                "key": "stale",
                "payload_hash": "0".repeat(64),
                "staged": 0,
                "accepted_at": Utc::now(),
            })
            .to_string(),
            created_at: Utc::now(),
        })
        .await?;

    let mut stale = envelope.clone();
    stale.caller_key = "capture-stale-version".to_owned();
    let version_refusal = harness.writer.apply_write_envelope(stale).await;
    assert!(
        matches!(
            version_refusal,
            Err(WriteEnvelopeError::VersionMismatch { .. })
        ),
        "a receipt from another envelope version must be refused, got {version_refusal:?}"
    );
    Ok(())
}

#[tokio::test]
async fn stale_expected_revision_persists_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("precondition").await?;
    let at = Utc::now();
    let source = source("precondition bytes");
    let claim = claim("zed", at);
    let previous = belief("zed", at);
    harness.epistemic.store_belief(previous.clone()).await?;

    let mut current = previous.clone();
    current.current_value = "helix".to_owned();
    current.updated_at = Utc::now();
    let envelope = envelope_with(
        "capture-precondition",
        Some(source.clone()),
        None,
        Some(claim.clone()),
        Some(previous.clone()),
        Some(current),
        vec![supersede_revision(&previous, &belief("helix", at))],
        Some("2020-01-01T00:00:00.000000000Z".to_owned()),
    );

    let refused = harness.writer.apply_write_envelope(envelope).await;
    assert!(
        matches!(refused, Err(WriteEnvelopeError::RevisionConflict { .. })),
        "a stale expected revision must be refused, got {refused:?}"
    );
    assert!(
        harness.sources.get(source.id()).await?.is_none(),
        "the precondition must be checked before the Source is stored"
    );
    assert!(
        harness.epistemic.get_claim(&claim.id).await?.is_none(),
        "the precondition must be checked before the Claim is stored"
    );
    assert_eq!(
        harness
            .epistemic
            .list_belief_revisions(&previous.id)
            .await?
            .len(),
        0,
        "the precondition must be checked before any revision exists"
    );
    Ok(())
}

#[tokio::test]
async fn envelope_refuses_blank_key_bad_separator_and_tampered_source()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("refusals").await?;
    let at = Utc::now();

    let blank = envelope_with(
        "   ",
        None,
        None,
        Some(claim("zed", at)),
        None,
        None,
        Vec::new(),
        None,
    );
    assert!(matches!(
        harness.writer.apply_write_envelope(blank).await,
        Err(WriteEnvelopeError::BlankKey)
    ));

    let separated = envelope_with(
        "a|b",
        None,
        None,
        Some(claim("zed", at)),
        None,
        None,
        Vec::new(),
        None,
    );
    assert!(matches!(
        harness.writer.apply_write_envelope(separated).await,
        Err(WriteEnvelopeError::KeySeparator)
    ));

    assert!(
        harness.epistemic.list_claims(false).await?.is_empty(),
        "refused envelopes must not write anything"
    );
    Ok(())
}

#[tokio::test]
async fn native_envelope_transaction_rolls_back_every_stage()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("staging").await?;
    let at = Utc::now();
    let source = source("staging bytes");
    let claim = claim("zed", at);
    let previous = belief("zed", at);
    harness.epistemic.store_belief(previous.clone()).await?;

    // A pre-existing revision ID makes the projection statement fail inside
    // the single correction transaction.
    let mut current = previous.clone();
    current.current_value = "helix".to_owned();
    current.updated_at = Utc::now();
    let existing = supersede_revision(&previous, &current);
    harness
        .epistemic
        .apply_belief_transition(
            Some(previous.clone()),
            Some(current.clone()),
            vec![existing.clone()],
            trace(),
            Vec::new(),
        )
        .await?;

    // Baseline: the setup transition above already committed one revision.
    let baseline_revisions = harness
        .epistemic
        .list_belief_revisions(&previous.id)
        .await?
        .len();
    assert_eq!(
        baseline_revisions, 1,
        "setup must commit exactly one revision"
    );

    let committed = existing_projection(&harness, &previous).await?;
    assert_eq!(
        committed.current_value, "helix",
        "setup must commit the superseding projection"
    );
    let mut attempted = committed.clone();
    attempted.current_value = "vim".to_owned();
    attempted.updated_at = Utc::now();

    let envelope = envelope_with(
        "capture-staging",
        Some(source.clone()),
        None,
        Some(claim.clone()),
        Some(committed),
        Some(attempted),
        vec![existing.clone()],
        None,
    );

    let result = harness.writer.apply_write_envelope(envelope.clone()).await;
    assert!(
        matches!(
            result,
            Err(WriteEnvelopeError::StageRejected { ref stage, .. })
                if stage == "envelope.transaction"
        ),
        "a rolled-back transaction must be reported as a failed stage: {result:?}"
    );

    // Q18 retirement proof: NOTHING earlier survived the failed projection —
    // the retained partial-commit risk is gone, not merely recorded.
    assert!(
        harness.sources.get(source.id()).await?.is_none(),
        "the whole transaction must roll the Source back with everything else"
    );
    assert!(
        harness.epistemic.get_claim(&claim.id).await?.is_none(),
        "the whole transaction must roll the Claim back with everything else"
    );

    assert_eq!(
        harness
            .epistemic
            .list_belief_revisions(&previous.id)
            .await?
            .len(),
        baseline_revisions,
        "the rolled-back projection must add no revision"
    );

    let key = envelope.key();
    assert!(
        harness
            .epistemic
            .get_correction_idempotency(&key)
            .await?
            .is_none(),
        "a failed envelope must leave no success receipt"
    );
    assert!(
        harness.writer.apply_write_envelope(envelope).await.is_err(),
        "a retry of the failed envelope must still report failure"
    );
    assert!(
        harness
            .epistemic
            .get_correction_idempotency(&key)
            .await?
            .is_none(),
        "a failed retry must leave no success receipt"
    );
    assert_eq!(
        existing_projection(&harness, &previous)
            .await?
            .current_value,
        "helix"
    );

    assert!(
        NATIVE_PARTIAL_COMMIT_RISK.contains("RETIRED"),
        "the retirement must be recorded in the adapter, loudly"
    );
    Ok(())
}
async fn existing_projection(
    harness: &Harness,
    previous: &Belief,
) -> Result<Belief, Box<dyn std::error::Error>> {
    harness
        .epistemic
        .get_belief(&previous.id)
        .await?
        .ok_or_else(|| "projection must exist".into())
}
