//! Q18 red half — correction-consistency gap fixture (LK-Q18).
//!
//! This file proves, on `integration/lantern-a01` @ `22d96d1`, that the
//! native correction path (`EpistemicService::record_correction`) does not
//! yet honour the correction-consistency contract. Every named test below is
//! RED on that base; the Q18 implementation seam (built on
//! `q17/hosted-capture` @ `686a3e0`) turns them green.
//!
//! The five proven gaps:
//!
//! 1. **Declared-target divergence** — the correction acts on whatever
//!    belief `reconcile_claim` re-discovers as newest for the key, not on
//!    the belief the caller declared: projection, result and revisions can
//!    all reference a different belief than the target.
//! 2. **Stale suppression** — a dependency-stale projection (stale flag,
//!    state Active) still accepts corrections; the contract says stale
//!    material is suppressed from ordinary operations.
//! 3. **Conflict-loser mutation** — the expected-revision check is a
//!    read-then-check-then-write sequence with no token binding at the
//!    write: a writer that read token T0 before the winner committed still
//!    applies and overwrites the winner (conflict losers must persist
//!    nothing; the token must be bound at the write).
//!
//! Clause mapping (shared vocabulary with the Q10 matrix and DeepSeek's Q19
//! fixture, so the three fixtures report as one matrix):
//!
//! | Q18 test | Q10/Q19 clause |
//! | --- | --- |
//! | `correction_acts_on_the_declared_target` | `value_history` + evidence alignment |
//! | `stale_projection_is_not_correctable` | `stale_suppression` (write refusal) |
//! | `stale_token_writer_must_not_mutate_the_projection` | CAS-refusal (nothing partial) |
//! | `replayed_correction_does_not_duplicate_records` | `replay_idempotent` |
//! | `failed_correction_leaves_no_partial_writes` | rollback — Q19 deliberately does not cover hosted mid-batch failure |
//! 4. **Replay duplication** — replaying an identical keyless correction
//!    (target by query) records a second claim, a second revision and a
//!    second evidence chain for the same correction.
//! 5. **No rollback** — a failure after the projection write (here: the
//!    idempotency receipt store) leaves every earlier write persisted:
//!    evidence source, claim, projection advance and revision all survive a
//!    correction the caller was told failed.
//!
//! The fixture is deliberately isolated: one new test file, an embedded
//! store, a failure-injecting repository wrapper. It edits no shared
//! Worker/MCP/D1 code.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use lighting_core::{
    Belief, BeliefId, EpistemicRepository, NewBelief, Scope, SourceRepository, Trace, TrustClass,
};
use lighting_service::EpistemicService;
use lighting_service::epistemic_dto::CorrectionRequest;
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealMemoryPathRepository, SurrealSourceRepository,
    SurrealStore,
};
use uuid::Uuid;

type HarnessParts = (
    Arc<SurrealEpistemicRepository>,
    Arc<SurrealSourceRepository>,
    Arc<SurrealMemoryPathRepository>,
);

async fn harness(label: &str) -> Result<HarnessParts, Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("lantern-q18-{label}-{}", Uuid::new_v4().simple()));
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "q18_correction".to_owned();
    config.database = "gap".to_owned();
    config.username.clear();
    config.password.clear();

    let store = SurrealStore::connect(&config).await?;
    store.initialise_schema().await?;
    let epistemic = SurrealEpistemicRepository::new(store.clone());
    epistemic.migrate().await?;
    let sources = SurrealSourceRepository::new(store.clone());
    sources.migrate().await?;
    let memory_paths = SurrealMemoryPathRepository::new(store);
    memory_paths.migrate().await?;
    Ok((
        Arc::new(epistemic),
        Arc::new(sources),
        Arc::new(memory_paths),
    ))
}

fn service(parts: &HarnessParts) -> EpistemicService {
    EpistemicService::new_with_evidence(parts.0.clone(), parts.1.clone(), parts.2.clone())
}

fn scope() -> Scope {
    BTreeMap::from([("project".to_owned(), "q18".to_owned())])
}

async fn seed_belief(
    repo: &SurrealEpistemicRepository,
    value: &str,
    at: chrono::DateTime<Utc>,
) -> Result<Belief, Box<dyn std::error::Error>> {
    let mut belief = Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "q18.editor".to_owned(),
        predicate_key: "preferred_editor".to_owned(),
        current_value: value.to_owned(),
        scope: scope(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: at,
        valid_from: Some(at),
        created_at: at,
    })
    .map_err(|error| format!("belief construction failed: {error}"))?;
    belief.updated_at = at;
    Ok(repo.store_belief(belief).await?)
}

fn request(
    target: Option<&BeliefId>,
    query: Option<&str>,
    value: &str,
    expected_revision: Option<String>,
    idempotency_key: Option<&str>,
) -> CorrectionRequest {
    CorrectionRequest {
        target_belief_id: target.map(|id| id.to_string()),
        target_query: query.map(str::to_owned),
        correction_text: format!("Q18: adopt {value} as the corrected value"),
        replacement_value: value.to_owned(),
        context_pack_id: None,
        scope: BTreeMap::new(),
        idempotency_key: idempotency_key.map(str::to_owned),
        expected_revision,
    }
}

/// Gap 1: the correction must act on the belief the caller declared as the
/// target — projection, reconciliation result and revisions may not drift to
/// a different belief that happens to share the key.
#[tokio::test]
async fn correction_acts_on_the_declared_target() -> Result<(), Box<dyn std::error::Error>> {
    let parts = harness("alignment").await?;
    let repo = &parts.0;
    let base = Utc::now() - Duration::minutes(5);
    // Two active projections with the same (holder, subject, predicate,
    // scope) key: the older one is the declared correction target.
    let older = seed_belief(repo, "alpha", base).await?;
    let newer = seed_belief(repo, "beta", base + Duration::minutes(1)).await?;

    let result = service(&parts)
        .record_correction(request(Some(&older.id), None, "gamma", None, None))
        .await?;

    let stored = repo
        .get_belief(&older.id)
        .await?
        .ok_or("declared target disappeared")?;
    assert_eq!(
        stored.state,
        lighting_core::BeliefState::Superseded,
        "the declared target must be the projection the correction superseded"
    );
    let untouched = repo
        .get_belief(&newer.id)
        .await?
        .ok_or("sibling projection disappeared")?;
    assert_eq!(
        untouched.current_value, "beta",
        "a projection the caller did not target must not absorb the correction"
    );
    assert_eq!(
        untouched.state,
        lighting_core::BeliefState::Active,
        "a projection the caller did not target must stay active"
    );
    let replacement = result
        .reconciliation
        .belief
        .as_ref()
        .ok_or("reconciliation produced no belief")?;
    assert_eq!(
        replacement.current_value, "gamma",
        "the correction's replacement must carry the new value"
    );
    assert_eq!(
        replacement.lineage.prior_belief_id.as_deref(),
        Some(older.id.as_str()),
        "the replacement must derive from the declared target, not a sibling"
    );
    Ok(())
}

/// Gap 2: stale projections are suppressed from ordinary operations and must
/// refuse corrections.
#[tokio::test]
async fn stale_projection_is_not_correctable() -> Result<(), Box<dyn std::error::Error>> {
    let parts = harness("stale").await?;
    let repo = &parts.0;
    let belief = seed_belief(repo, "alpha", Utc::now()).await?;
    repo.mark_belief_stale(&belief.id, "dependency moved", Utc::now())
        .await?;

    let outcome = service(&parts)
        .record_correction(request(Some(&belief.id), None, "gamma", None, None))
        .await;
    assert!(
        outcome.is_err(),
        "a stale projection must refuse corrections, got: {outcome:?}"
    );
    Ok(())
}

/// Gap 3: a conflict loser must not mutate the projection — the revision
/// token has to be bound AT THE WRITE, not only checked before it. The
/// service pre-checks a freshly read token, so the race window is proven at
/// the write seam: a writer that read token T0 before the winner committed
/// must be refused, not silently overwrite the winner.
#[tokio::test]
async fn stale_token_writer_must_not_mutate_the_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let parts = harness("conflict").await?;
    let repo = &parts.0;
    let original = seed_belief(repo, "alpha", Utc::now()).await?;

    // The loser read the projection (token T0) before the winner committed.
    let mut loser = original.clone();
    loser.current_value = "loser".to_owned();

    // The winner commits first: alpha -> winner, token advances to T1.
    let mut winner = original.clone();
    winner.current_value = "winner".to_owned();
    winner.updated_at = original.updated_at + Duration::seconds(1);
    repo.apply_belief_transition_guarded(
        Some(original.clone()),
        Some(winner),
        Vec::new(),
        conflict_trace(),
        Vec::new(),
        Some(original.updated_at),
    )
    .await?;

    // The loser now applies with its stale token — this must be refused.
    let outcome = repo
        .apply_belief_transition_guarded(
            Some(loser.clone()),
            Some(loser),
            Vec::new(),
            conflict_trace(),
            Vec::new(),
            Some(original.updated_at),
        )
        .await;
    let stored = repo
        .get_belief(&original.id)
        .await?
        .ok_or("projection disappeared")?;
    assert!(
        outcome.is_err(),
        "a write bound to a consumed token must be refused, but apply returned Ok (stored value: {})",
        stored.current_value
    );
    assert_eq!(
        stored.current_value, "winner",
        "a conflict loser must not overwrite the winner's projection"
    );
    Ok(())
}

fn conflict_trace() -> Trace {
    Trace {
        id: lighting_core::TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        event_type: "q18_conflict_loser".to_owned(),
        actor_id: "matthew".to_owned(),
        subject_id: None,
        input_ids: Vec::new(),
        output_ids: Vec::new(),
        details: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

/// Gap 4: replaying an identical keyless correction must not record a second
/// claim, revision or evidence chain for the same correction.
#[tokio::test]
async fn replayed_correction_does_not_duplicate_records() -> Result<(), Box<dyn std::error::Error>>
{
    let parts = harness("replay").await?;
    let repo = &parts.0;
    let belief = seed_belief(repo, "alpha", Utc::now()).await?;

    let first = service(&parts)
        .record_correction(request(None, Some("q18.editor"), "gamma", None, None))
        .await?;
    let second = service(&parts)
        .record_correction(request(None, Some("q18.editor"), "gamma", None, None))
        .await?;

    let claims = repo.list_claims(false).await?;
    let gamma_claims = claims.iter().filter(|claim| claim.value == "gamma").count();
    assert_eq!(
        gamma_claims, 1,
        "a replayed correction must not create a second claim (first={}, second={})",
        first.claim.id, second.claim.id
    );

    let target_id = first
        .reconciliation
        .belief
        .as_ref()
        .map(|belief| belief.id.clone())
        .unwrap_or(belief.id.clone());
    let revisions = repo.list_belief_revisions(&target_id).await?;
    assert_eq!(
        revisions.len(),
        1,
        "a replayed correction must not append a second revision"
    );
    Ok(())
}

/// Gap 5: a correction that fails after the projection write must roll every
/// earlier write back — evidence source, claim, projection and revisions.
#[tokio::test]
async fn failed_correction_leaves_no_partial_writes() -> Result<(), Box<dyn std::error::Error>> {
    let parts = harness("rollback").await?;
    let repo = &parts.0;
    let belief = seed_belief(repo, "alpha", Utc::now()).await?;
    let claims_before = repo.list_claims(false).await?.len();
    let revisions_before = repo.list_belief_revisions(&belief.id).await?.len();

    // Natural mid-batch failure: the test seam THROWs inside the single
    // correction transaction AFTER every earlier statement executed, so the
    // rollback proven below is the transaction's real rollback.
    lighting_store_surreal::epistemic_store::Q18_FAULT_INJECT.with(|flag| flag.set(true));
    let outcome = service(&parts)
        .record_correction(request(
            Some(&belief.id),
            None,
            "gamma",
            None,
            Some("q18-rollback"),
        ))
        .await;
    lighting_store_surreal::epistemic_store::Q18_FAULT_INJECT.with(|flag| flag.set(false));
    assert!(
        outcome.is_err(),
        "the injected mid-batch failure must surface to the caller, got: {outcome:?}"
    );

    let claims_after = repo.list_claims(false).await?.len();
    assert_eq!(
        claims_after, claims_before,
        "a failed correction must not leave its claim persisted"
    );
    let stored = repo
        .get_belief(&belief.id)
        .await?
        .ok_or("target disappeared")?;
    assert_eq!(
        stored.current_value, "alpha",
        "a failed correction must not advance the projection"
    );
    let revisions_after = repo.list_belief_revisions(&belief.id).await?.len();
    assert_eq!(
        revisions_after, revisions_before,
        "a failed correction must not leave a revision behind"
    );
    let evidence_title = lighting_core::SourceTitle::new("Matthew correction")?;
    let evidence_sources = parts
        .1
        .list_all_by_kind_and_title(lighting_core::SourceKind::PlainText, &evidence_title)
        .await?;
    assert!(
        evidence_sources.is_empty(),
        "a failed correction must not leave its evidence source persisted ({} found)",
        evidence_sources.len()
    );
    Ok(())
}
