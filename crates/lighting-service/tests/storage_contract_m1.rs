//! M1 storage-seam contract tests for LK-HOST-001.
//!
//! These tests pin the behaviour a hosted storage adapter must reproduce, using
//! the **native embedded SurrealKV store** as the reference implementation.
//! They are deliberately adapter-facing: no D1, no network, no HTTP service
//! surface, no Cloudflare credentials.
//!
//! Contract cases covered here:
//!
//! 1. scoped typed identifiers round-trip unchanged (scope participates in identity);
//! 2. primary read-after-write for claims and beliefs;
//! 3. a rolled-back projection transition leaves no orphan revision and no
//!    partially applied belief update;
//! 4. idempotent replay — a repeated key yields a duplicate receipt and one record;
//! 5. two concurrent writers never leave a torn projection or an unattributable revision;
//! 6. the unbounded `list_*` trait shape, measured at 1k+ records.
//!
//! Guarantees the native implementation does **not** currently provide are pinned
//! as `#[ignore]` tests with an explicit, measured reason. Running
//!
//! ```text
//! cargo test -p lighting-service --test storage_contract_m1 -- --ignored --test-threads=1
//! ```
//!
//! reproduces each one. Nothing here changes `EpistemicRepository` or the store;
//! the gaps are recorded so M1 can decide between a bounded adapter contract and
//! a service-side change.
//!
//! ### Measured defects at M1 base ac619cca (native embedded SurrealKV)
//!
//! * `apply_belief_transition` returns `Ok(current)` after issuing a
//!   `BEGIN … COMMIT` batch without inspecting per-statement results. A batch
//!   that rolls back is therefore reported to the caller as an accepted write.
//! * Two concurrent transitions on one belief can both return `Ok` while only one
//!   revision persists, so a writer can be told its correction was accepted when
//!   no evidence for it exists. There is no compare-and-set precondition.
//!
//! Both are false-acceptance risks a D1 adapter must not inherit: an accepted
//! write must be durable, and a rejected one must not look accepted.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use chrono::{DateTime, Utc};
use lighting_core::{
    Belief, BeliefId, BeliefRevision, BeliefRevisionId, BoundedEpistemicRepository, BoundedError,
    Claim, ClaimId, EpistemicRepository, IdempotentTransition, LedgerEvent, LedgerEventRepository,
    LedgerIngestResult, LedgerRole, MAX_PAGE_LIMIT, MAX_TRAVERSAL_DEPTH, MAX_TRAVERSAL_FANOUT,
    NewBelief, NewClaim, PageCursor, PageRequest, PredicateStatus, ReconciliationAction, Scope,
    Stance, Trace, TraceId, TraversalBounds, TrustClass, WriteOutcome, scope_hash,
};
use lighting_store_surreal::{
    StoreConfig, SurrealBoundedRepository, SurrealEpistemicRepository, SurrealLedgerRepository,
    SurrealStore,
};
use uuid::Uuid;

const NAMESPACE: &str = "m1_storage_contract";

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-m1-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

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

async fn epistemic_harness(
    label: &str,
) -> Result<SurrealEpistemicRepository, Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&embedded_config(test_path(label))).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    Ok(repository)
}

fn scope(project: &str) -> Scope {
    BTreeMap::from([
        ("project".to_owned(), project.to_owned()),
        ("holder".to_owned(), "pi".to_owned()),
    ])
}

fn new_claim(
    subject: &str,
    predicate: &str,
    value: &str,
    scope: Scope,
    at: DateTime<Utc>,
) -> Claim {
    Claim::new(NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: subject.to_owned(),
        predicate_key: Some(predicate.to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: value.to_owned(),
        scope,
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("pi".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: at,
        valid_from: Some(at),
        valid_to: None,
        extractor: "m1-contract".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: at,
    })
    .expect("claim fixture must construct")
}

fn new_belief(
    subject: &str,
    predicate: &str,
    value: &str,
    scope: Scope,
    at: DateTime<Utc>,
) -> Belief {
    Belief::new(NewBelief {
        holder_key: "pi".to_owned(),
        subject_key: subject.to_owned(),
        predicate_key: predicate.to_owned(),
        current_value: value.to_owned(),
        scope,
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: at,
        valid_from: Some(at),
        created_at: at,
    })
    .expect("belief fixture must construct")
}

fn trace(event_type: &str) -> Trace {
    Trace {
        id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        event_type: event_type.to_owned(),
        actor_id: "pi".to_owned(),
        subject_id: None,
        input_ids: Vec::new(),
        output_ids: Vec::new(),
        details: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn revision(
    belief_id: &BeliefId,
    previous_value: Option<&str>,
    new_value: &str,
    trace_id: &TraceId,
) -> BeliefRevision {
    BeliefRevision {
        id: BeliefRevisionId::new(Uuid::new_v4().to_string()).expect("revision id"),
        belief_id: belief_id.clone(),
        previous_value: previous_value.map(str::to_owned),
        new_value: Some(new_value.to_owned()),
        claim_id: None,
        transition_type: ReconciliationAction::Supersede,
        trace_id: trace_id.clone(),
        created_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// 1. Scoped typed identifiers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scoped_typed_ids_round_trip_and_scope_participates_in_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = epistemic_harness("scoped-ids").await?;
    let at = Utc::now();
    let claim = new_claim(
        "matthew",
        "preferred_editor",
        "zed",
        scope("lantern-keeper"),
        at,
    );

    let stored = repository.store_claim(claim.clone()).await?;
    assert_eq!(stored.id, claim.id, "stores must not rewrite the typed id");

    let fetched = repository
        .get_claim(&claim.id)
        .await?
        .expect("claim must be readable by its typed id");
    assert_eq!(
        fetched.id.as_str(),
        claim.id.as_str(),
        "typed ClaimId must round-trip as the same value"
    );
    assert_eq!(fetched.scope, claim.scope, "scope must round-trip");
    assert_eq!(
        fetched.scope_hash,
        scope_hash(&claim.scope),
        "stored scope_hash must be the canonical hash of the stored scope"
    );
    assert_eq!(fetched.dedupe_key, claim.dedupe_key);

    // The same statement in a different project scope is a different record:
    // scope is part of claim identity through the dedupe key and scope hash.
    let other_scope_claim = new_claim(
        "matthew",
        "preferred_editor",
        "zed",
        scope("hosted-memory"),
        at,
    );
    assert_ne!(
        other_scope_claim.id, claim.id,
        "distinct scoped claims need distinct typed ids"
    );
    assert_ne!(
        other_scope_claim.dedupe_key, claim.dedupe_key,
        "scope must not be discarded from claim identity"
    );
    repository.store_claim(other_scope_claim.clone()).await?;

    let claims = repository.list_claims(false).await?;
    assert_eq!(claims.len(), 2, "both scoped claims must persist");
    let lantern_only: Vec<_> = claims
        .iter()
        .filter(|claim| claim.scope.get("project").map(String::as_str) == Some("lantern-keeper"))
        .collect();
    assert_eq!(lantern_only.len(), 1, "scope filtering must be unambiguous");
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Primary read-after-write
// ---------------------------------------------------------------------------

#[tokio::test]
async fn primary_read_after_write_returns_the_stored_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = epistemic_harness("read-after-write").await?;
    let at = Utc::now();

    let claim = new_claim(
        "matthew",
        "timezone",
        "Europe/London",
        scope("workshop"),
        at,
    );
    repository.store_claim(claim.clone()).await?;
    assert_eq!(
        repository.get_claim(&claim.id).await?,
        Some(claim.clone()),
        "a claim read back immediately after write must be identical"
    );

    let belief = new_belief(
        "matthew",
        "timezone",
        "Europe/London",
        scope("workshop"),
        at,
    );
    repository.store_belief(belief.clone()).await?;
    assert_eq!(
        repository.get_belief(&belief.id).await?,
        Some(belief.clone()),
        "a belief read back immediately after write must be identical"
    );

    let missing = ClaimId::new(Uuid::new_v4().to_string())?;
    assert_eq!(
        repository.get_claim(&missing).await?,
        None,
        "an unknown id must be explicitly absent, not an error"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Rolled-back projection transition
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rolled_back_transition_leaves_no_orphan_revision() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = epistemic_harness("atomic-transition").await?;
    let at = Utc::now();
    let scope = scope("workshop");

    let first = new_belief("matthew", "preferred_editor", "zed", scope.clone(), at);
    repository.store_belief(first.clone()).await?;

    // A dependent belief that the same transaction should mark stale.
    let dependent = new_belief("matthew", "editor_theme", "solarized", scope.clone(), at);
    repository.store_belief(dependent.clone()).await?;

    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = at;

    let mut stale_dependent = dependent.clone();
    stale_dependent.mark_stale("dependency changed", at);
    stale_dependent.updated_at = at;

    let first_trace = trace("projection.supersede");
    let first_revision = revision(&first.id, Some("zed"), "helix", &first_trace.id);
    repository
        .apply_belief_transition(
            Some(first.clone()),
            Some(second.clone()),
            vec![first_revision.clone()],
            first_trace,
            vec![stale_dependent],
        )
        .await?;

    assert_eq!(
        repository.get_belief(&first.id).await?,
        Some(second.clone()),
        "the accepted transition must be readable immediately"
    );
    assert_eq!(
        repository.list_belief_revisions(&first.id).await?.len(),
        1,
        "the accepted transition must record exactly one revision"
    );
    assert!(
        repository
            .get_belief(&dependent.id)
            .await?
            .expect("dependent belief")
            .stale,
        "stale propagation must be applied by the same transition"
    );

    // Inject a mid-batch failure: the revision id already exists, so the
    // revision CREATE inside the BEGIN/COMMIT batch cannot succeed.
    let mut third = second.clone();
    third.current_value = "vim".to_owned();
    third.updated_at = Utc::now();
    let failing_trace = trace("projection.supersede.failing");
    let duplicate_revision = BeliefRevision {
        id: first_revision.id.clone(),
        created_at: Utc::now(),
        ..revision(&first.id, Some("helix"), "vim", &failing_trace.id)
    };
    let mut should_not_be_applied = dependent.clone();
    should_not_be_applied.stale_reason = Some("must not be written".to_owned());
    should_not_be_applied.updated_at = Utc::now();

    let result = repository
        .apply_belief_transition(
            Some(second.clone()),
            Some(third.clone()),
            vec![duplicate_revision],
            failing_trace,
            vec![should_not_be_applied],
        )
        .await;

    // Rollback invariants. These hold on the native store today; the missing
    // failure signal is pinned separately by the ignored test below.
    let rolled_back = result.is_err();
    eprintln!(
        "m1 contract: duplicate-revision transition reported rollback={rolled_back} (Ok means \
         the failure was not surfaced to the caller)"
    );
    assert_eq!(
        repository.get_belief(&first.id).await?,
        Some(second),
        "a failed batch must not persist its belief update (no partial commit)"
    );
    let revisions = repository.list_belief_revisions(&first.id).await?;
    assert_eq!(
        revisions.len(),
        1,
        "a failed batch must leave no orphan revision, found {revisions:?}"
    );
    assert_eq!(
        revisions[0].id, first_revision.id,
        "the surviving revision must be the accepted one, not the rejected one"
    );
    assert_eq!(
        revisions[0].new_value.as_deref(),
        Some("helix"),
        "a rejected revision must not overwrite an accepted revision payload"
    );
    let dependent_after = repository
        .get_belief(&dependent.id)
        .await?
        .expect("dependent belief");
    assert_eq!(
        dependent_after.stale_reason.as_deref(),
        Some("dependency changed"),
        "a failed batch must not rewrite the stale reason of another belief"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "re-pointed: the existing native apply_belief_transition still returns Ok for a \
            rolled-back batch, and is intentionally unchanged by this slice. The guarantee now \
            lives on the additive bounded surface, proven by \
            rejected_statement_surfaces_as_failure_not_success."]
async fn rolled_back_transition_must_not_report_success() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = epistemic_harness("false-accept").await?;
    let at = Utc::now();
    let first = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);
    repository.store_belief(first.clone()).await?;

    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = Utc::now();
    let t1 = trace("projection.supersede");
    let r1 = revision(&first.id, Some("zed"), "helix", &t1.id);
    repository
        .apply_belief_transition(
            Some(first.clone()),
            Some(second.clone()),
            vec![r1.clone()],
            t1,
            Vec::new(),
        )
        .await?;

    let mut third = second.clone();
    third.current_value = "vim".to_owned();
    third.updated_at = Utc::now();
    let t2 = trace("projection.supersede.conflicting");
    let duplicate = BeliefRevision {
        id: r1.id.clone(),
        created_at: Utc::now(),
        ..revision(&first.id, Some("helix"), "vim", &t2.id)
    };
    let result = repository
        .apply_belief_transition(
            Some(second.clone()),
            Some(third),
            vec![duplicate],
            t2,
            Vec::new(),
        )
        .await;
    assert!(
        result.is_err(),
        "a rolled-back batch must be reported as a failure, not as an accepted write"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Idempotent replay
// ---------------------------------------------------------------------------

#[tokio::test]
async fn repeated_key_yields_duplicate_receipt_without_duplicating_records()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = epistemic_harness("replay-claim").await?;
    let at = Utc::now();

    // Claims carry a deterministic dedupe key, so an identical replay is
    // recognised even though each constructed Claim has a fresh typed id.
    let original = new_claim("matthew", "preferred_editor", "zed", scope("workshop"), at);
    let replayed = new_claim("matthew", "preferred_editor", "zed", scope("workshop"), at);
    assert_ne!(original.id, replayed.id, "fixture ids must differ");
    assert_eq!(
        original.dedupe_key, replayed.dedupe_key,
        "identical content must derive the same replay key"
    );

    let first = repository.store_claim(original.clone()).await?;
    let second = repository.store_claim(replayed).await?;
    assert_eq!(
        second.id, first.id,
        "a replayed claim must return the original record, not a new one"
    );
    assert_eq!(
        repository.list_claims(false).await?.len(),
        1,
        "replay must not duplicate the stored claim"
    );
    Ok(())
}

#[tokio::test]
async fn ledger_ingest_replay_reports_duplicate_without_duplicating_events()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&embedded_config(test_path("replay-ledger"))).await?;
    store.initialise_schema().await?;
    let ledger = SurrealLedgerRepository::new(store);
    ledger.migrate().await?;

    let event = LedgerEvent {
        event_id: "m1:turn:1".to_owned(),
        source: "m1-contract".to_owned(),
        external_id: Some("turn-1".to_owned()),
        session_id: Some("session-1".to_owned()),
        conversation_id: None,
        turn_id: Some("turn-1".to_owned()),
        actor: "matthew".to_owned(),
        role: LedgerRole::User,
        content: "Remember this decision.".to_owned(),
        observed_at: None,
        received_at: Utc::now(),
        reply_to: None,
        project_hint: Some("lantern-keeper".to_owned()),
        idempotency_key: "m1:turn:1".to_owned(),
        raw_payload: None,
        metadata: BTreeMap::new(),
    };

    match ledger.ingest(event.clone()).await? {
        LedgerIngestResult::Stored(stored) => assert_eq!(stored.event_id, event.event_id),
        LedgerIngestResult::Duplicate(_) => panic!("the first ingest must be stored"),
    }
    match ledger.ingest(event.clone()).await? {
        LedgerIngestResult::Duplicate(duplicate) => {
            assert_eq!(
                duplicate.event_id, event.event_id,
                "the duplicate receipt must name the original event identity"
            );
        }
        LedgerIngestResult::Stored(_) => panic!("the replayed ingest must report a duplicate"),
    }

    let listed = ledger.list(Some("m1-contract")).await?;
    assert_eq!(
        listed.len(),
        1,
        "replay must not duplicate ledger events, found {listed:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. Two concurrent writers
// ---------------------------------------------------------------------------

struct ConcurrentOutcome {
    left_ok: bool,
    right_ok: bool,
    left_revision: BeliefRevisionId,
    right_revision: BeliefRevisionId,
    base_value: String,
    left_value: String,
    right_value: String,
    revisions: Vec<BeliefRevision>,
    current_value: String,
}

async fn run_concurrent_writers(
    repository: &SurrealEpistemicRepository,
    label: &str,
) -> Result<ConcurrentOutcome, Box<dyn std::error::Error>> {
    let at = Utc::now();
    let base = new_belief("matthew", "preferred_editor", "zed", scope(label), at);
    repository.store_belief(base.clone()).await?;

    let mut left = base.clone();
    left.current_value = "helix".to_owned();
    left.updated_at = Utc::now();
    let mut right = base.clone();
    right.current_value = "vim".to_owned();
    right.updated_at = Utc::now();

    let left_trace = trace("projection.supersede.left");
    let right_trace = trace("projection.supersede.right");
    let left_revision = revision(&base.id, Some("zed"), "helix", &left_trace.id);
    let right_revision = revision(&base.id, Some("zed"), "vim", &right_trace.id);

    let (left_result, right_result) = tokio::join!(
        repository.apply_belief_transition(
            Some(base.clone()),
            Some(left.clone()),
            vec![left_revision.clone()],
            left_trace,
            Vec::new(),
        ),
        repository.apply_belief_transition(
            Some(base.clone()),
            Some(right.clone()),
            vec![right_revision.clone()],
            right_trace,
            Vec::new(),
        ),
    );

    let revisions = repository.list_belief_revisions(&base.id).await?;
    let current = repository
        .get_belief(&base.id)
        .await?
        .expect("belief must survive concurrent writers");

    Ok(ConcurrentOutcome {
        left_ok: left_result.is_ok(),
        right_ok: right_result.is_ok(),
        left_revision: left_revision.id,
        right_revision: right_revision.id,
        base_value: base.current_value,
        left_value: left.current_value,
        right_value: right.current_value,
        revisions,
        current_value: current.current_value,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_writers_never_leave_a_torn_or_unattributable_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = epistemic_harness("concurrent-writers").await?;
    let outcome = run_concurrent_writers(&repository, "workshop").await?;

    eprintln!(
        "m1 contract: concurrent transitions -> left_ok={} right_ok={} revisions={} current={}",
        outcome.left_ok,
        outcome.right_ok,
        outcome.revisions.len(),
        outcome.current_value
    );

    assert!(
        outcome.left_ok || outcome.right_ok,
        "at least one concurrent transition must commit"
    );
    // A torn write would leave a value that no writer produced.
    assert!(
        [
            outcome.base_value.as_str(),
            outcome.left_value.as_str(),
            outcome.right_value.as_str(),
        ]
        .contains(&outcome.current_value.as_str()),
        "the surviving projection must be one complete value, found {}",
        outcome.current_value
    );
    // Every durable revision must belong to a writer that reported success.
    for revision in &outcome.revisions {
        let attributable = (outcome.left_ok && revision.id == outcome.left_revision)
            || (outcome.right_ok && revision.id == outcome.right_revision);
        assert!(
            attributable,
            "a durable revision must belong to a writer that reported success, found {}",
            revision.id
        );
    }
    assert!(
        !outcome.revisions.is_empty(),
        "a committed concurrent transition must leave durable evidence"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "re-pointed: the existing native apply_belief_transition still has no compare-and-set \
            precondition, so a losing concurrent writer is silently dropped. The guarantee now \
            lives on the additive bounded surface, proven by \
            concurrent_idempotent_transitions_commit_exactly_one."]
async fn every_concurrent_success_has_durable_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let repository = epistemic_harness("concurrent-false-accept").await?;
    let outcome = run_concurrent_writers(&repository, "workshop-race").await?;

    for (label, ok, revision) in [
        ("left", outcome.left_ok, &outcome.left_revision),
        ("right", outcome.right_ok, &outcome.right_revision),
    ] {
        if ok {
            assert!(
                outcome
                    .revisions
                    .iter()
                    .any(|candidate| &candidate.id == revision),
                "{label} reported success but has no durable revision"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Unbounded list_* contract at 1k+ records
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unbounded_list_contract_measures_1k_records() -> Result<(), Box<dyn std::error::Error>> {
    const CLAIMS: usize = 1_000;
    const BELIEFS: usize = 250;

    let repository = epistemic_harness("list-scale").await?;
    let at = Utc::now();

    let seed_started = Instant::now();
    for index in 0..CLAIMS {
        let claim = new_claim(
            &format!("subject-{}", index % 25),
            "preferred_editor",
            &format!("value-{index}"),
            scope("scale-fixture"),
            at,
        );
        repository.store_claim(claim).await?;
    }
    for index in 0..BELIEFS {
        let belief = new_belief(
            &format!("subject-{}", index % 25),
            "preferred_editor",
            &format!("belief-{index}"),
            scope("scale-fixture"),
            at,
        );
        repository.store_belief(belief).await?;
    }
    let seed_elapsed = seed_started.elapsed();

    let claims_started = Instant::now();
    let claims = repository.list_claims(false).await?;
    let claims_elapsed = claims_started.elapsed();

    let beliefs_started = Instant::now();
    let beliefs = repository.list_beliefs(true).await?;
    let beliefs_elapsed = beliefs_started.elapsed();

    eprintln!(
        "m1 storage contract: seeded {CLAIMS} claims + {BELIEFS} beliefs in {seed_elapsed:?}; \
         list_claims -> {} rows in {claims_elapsed:?}; list_beliefs(include_stale) -> {} rows in \
         {beliefs_elapsed:?}",
        claims.len(),
        beliefs.len()
    );

    // The trait takes no limit or page parameter for these two calls, so the
    // native contract is "return everything". A hosted adapter cannot match that
    // shape inside a D1 row budget; this pins the gap until a bounded variant is
    // designed.
    assert_eq!(
        claims.len(),
        CLAIMS,
        "list_claims currently returns every claim; a bounded adapter contract will change this"
    );
    assert_eq!(
        beliefs.len(),
        BELIEFS,
        "list_beliefs currently returns every matching belief"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 7. Recorded gaps — not expressible through the current trait
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "re-pointed: the native transition path still has no caller-supplied idempotency key. \
            The bounded surface provides one, proven by \
            idempotent_transition_replays_without_duplicate_records."]
async fn projection_transition_replay_is_keyed_by_caller() -> Result<(), Box<dyn std::error::Error>>
{
    // A retry of apply_belief_transition after a lost response creates the trace
    // and revisions again, because revision ids are generated by the caller and
    // there is no receipt key to deduplicate on.
    Ok(())
}

#[tokio::test]
#[ignore = "re-pointed: the native repository intentionally keeps its unbounded list_* methods for \
            local use. Cursor paging exists on the additive bounded surface, proven by \
            bounded_pages_are_keyset_ordered_without_omission."]
async fn list_calls_are_bounded_for_hosted_row_limits() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[tokio::test]
#[ignore = "measured at M1 base 2787b46: store_belief and SurrealEpistemicRepository::migrate \
            issue statements without inspecting per-statement results, so a rejected statement \
            can still look like success. Same false-acceptance family as \
            rolled_back_transition_must_not_report_success; the bounded surface checks \
            take_errors, these two native paths do not. Reported for a sequenced follow-up, not \
            drive-by fixed here."]
async fn native_store_belief_and_migrate_do_not_check_statement_results()
-> Result<(), Box<dyn std::error::Error>> {
    // No assertion is possible through the current native API: both calls return
    // Ok or an Err derived from the driver call, never from a per-statement
    // result. The bounded surface (bounded_store.rs) is the checked path.
    Ok(())
}

// ---------------------------------------------------------------------------
// 9. Additive bounded surface (LK-HOST-001 v1.1)
//
// These run against lighting_core::bounded / SurrealBoundedRepository, which is
// additive: the native repository, its unbounded list_* methods and the existing
// transition path are unchanged and still pinned above.
// ---------------------------------------------------------------------------

async fn bounded_harness(
    label: &str,
) -> Result<(SurrealEpistemicRepository, SurrealBoundedRepository), Box<dyn std::error::Error>> {
    let store = SurrealStore::connect(&embedded_config(test_path(label))).await?;
    store.initialise_schema().await?;
    let epistemic = SurrealEpistemicRepository::new(store.clone());
    epistemic.migrate().await?;
    let bounded = SurrealBoundedRepository::new(store);
    bounded.migrate().await?;
    Ok((epistemic, bounded))
}

fn transition(
    belief: Option<&Belief>,
    previous: Option<&Belief>,
    key: &str,
    expected_revision: Option<u64>,
    new_value: Option<&str>,
) -> IdempotentTransition {
    let current = match (new_value, previous) {
        (Some(value), Some(previous)) => {
            let mut next = previous.clone();
            next.current_value = value.to_owned();
            next.updated_at = Utc::now();
            Some(next)
        }
        _ => belief.cloned(),
    };
    let trace = trace("bounded.transition");
    let revisions = match (previous, new_value) {
        (Some(previous), Some(value)) => vec![revision(
            &previous.id,
            Some(previous.current_value.as_str()),
            value,
            &trace.id,
        )],
        _ => Vec::new(),
    };
    IdempotentTransition {
        principal_id: "pi".to_owned(),
        scope: scope("workshop"),
        idempotency_key: key.to_owned(),
        expected_revision,
        previous: previous.cloned(),
        current,
        revisions,
        trace,
        stale_beliefs: Vec::new(),
    }
}

#[tokio::test]
async fn bounded_pages_are_keyset_ordered_without_omission()
-> Result<(), Box<dyn std::error::Error>> {
    let (epistemic, bounded) = bounded_harness("bounded-pages").await?;
    let at = Utc::now();

    let mut claim_ids = Vec::new();
    for index in 0..25 {
        let claim = new_claim(
            &format!("subject-{}", index % 5),
            "preferred_editor",
            &format!("claim-{index}"),
            scope("pages"),
            at,
        );
        claim_ids.push(claim.id.to_string());
        epistemic.store_claim(claim).await?;
    }
    let mut belief_ids = Vec::new();
    for index in 0..21 {
        let belief = new_belief(
            &format!("subject-{}", index % 5),
            "preferred_editor",
            &format!("belief-{index}"),
            scope("pages"),
            at,
        );
        belief_ids.push(belief.id.to_string());
        epistemic.store_belief(belief).await?;
    }
    for index in 0..17 {
        let relation = lighting_core::GraphRelation {
            id: lighting_core::RelationId::new(Uuid::new_v4().to_string())?,
            in_id: format!("belief:{}", belief_ids[index % belief_ids.len()]),
            out_id: format!("claim:{}", claim_ids[index % claim_ids.len()]),
            relation_type: "supports".to_owned(),
            origin: "m1-contract".to_owned(),
            confidence: 0.9,
            resolved: true,
            created_at: at,
            dedupe_key: format!("rel-{index}"),
        };
        epistemic.store_relation(relation).await?;
    }

    let mut page_count = 0;
    let mut request = PageRequest::first(10)?;
    let mut total = 0;
    loop {
        let page = bounded.list_claims_page(request).await?;
        total += page.items().len();
        page_count += 1;
        match page.next_cursor() {
            Some(cursor) => request = PageRequest::after(cursor.clone(), 10)?,
            None => break,
        }
    }
    assert_eq!(
        total,
        claim_ids.len(),
        "every claim must appear exactly once"
    );
    assert!(
        page_count >= 3,
        "paging must actually page, saw {page_count}"
    );

    let mut belief_total = 0;
    let mut request = PageRequest::first(7)?;
    loop {
        let page = bounded.list_beliefs_page(true, request).await?;
        belief_total += page.items().len();
        match page.next_cursor() {
            Some(cursor) => request = PageRequest::after(cursor.clone(), 7)?,
            None => break,
        }
    }
    assert_eq!(
        belief_total,
        belief_ids.len(),
        "every belief must appear exactly once"
    );

    let mut relation_total = 0;
    let mut request = PageRequest::first(5)?;
    loop {
        let page = bounded.list_relations_page(request).await?;
        relation_total += page.items().len();
        match page.next_cursor() {
            Some(cursor) => request = PageRequest::after(cursor.clone(), 5)?,
            None => break,
        }
    }
    assert_eq!(
        relation_total, 17,
        "every relation must appear exactly once"
    );

    // A page that returns fewer than the limit must not offer a cursor.
    let first = bounded
        .list_claims_page(PageRequest::first(MAX_PAGE_LIMIT)?)
        .await?;
    assert_eq!(first.items().len(), claim_ids.len());
    assert!(
        first.next_cursor().is_none(),
        "an exhausted page must not offer a cursor"
    );
    Ok(())
}

#[tokio::test]
async fn bounded_page_rejects_invalid_cursor_and_limit() -> Result<(), Box<dyn std::error::Error>> {
    let (_epistemic, bounded) = bounded_harness("bounded-refusals").await?;

    assert_eq!(
        PageRequest::first(0).expect_err("zero limit must be refused"),
        BoundedError::LimitOutOfRange { limit: 0 }
    );
    assert_eq!(
        PageRequest::first(MAX_PAGE_LIMIT + 1).expect_err("oversized limit must be refused"),
        BoundedError::LimitOutOfRange {
            limit: MAX_PAGE_LIMIT + 1
        }
    );

    let forged = PageCursor::from("Zm9yZ2Vk".to_owned());
    let result = bounded
        .list_claims_page(PageRequest::after(forged, 10)?)
        .await;
    assert_eq!(
        result.expect_err("a cursor this server did not issue must be refused"),
        BoundedError::InvalidCursor
    );
    Ok(())
}

#[tokio::test]
async fn idempotent_transition_replays_without_duplicate_records()
-> Result<(), Box<dyn std::error::Error>> {
    let (epistemic, bounded) = bounded_harness("bounded-replay").await?;
    let at = Utc::now();

    let first_transition = transition(None, None, "key-replay", None, None);
    let mut seed = first_transition.clone();
    let belief = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);
    seed.current = Some(belief.clone());
    seed.previous = None;

    let accepted = bounded.apply_idempotent_transition(seed.clone()).await?;
    assert_eq!(
        accepted.outcome,
        WriteOutcome::Accepted { revision_seq: 1 },
        "the first write must be accepted at revision 1"
    );
    let replayed = bounded.apply_idempotent_transition(seed.clone()).await?;
    assert_eq!(
        replayed.outcome,
        WriteOutcome::Replayed,
        "an exact retry must replay instead of writing again"
    );
    assert_eq!(
        replayed.receipt_id, accepted.receipt_id,
        "a replay must recover the original receipt"
    );
    assert_eq!(
        epistemic.list_belief_revisions(&belief.id).await?.len(),
        0,
        "replay must not duplicate revisions"
    );
    assert_eq!(
        epistemic.list_beliefs(true).await?.len(),
        1,
        "replay must not duplicate the projection"
    );

    let mut changed = seed.clone();
    changed.current = Some({
        let mut other = belief.clone();
        other.current_value = "helix".to_owned();
        other
    });
    assert!(
        matches!(
            bounded.apply_idempotent_transition(changed).await,
            Err(BoundedError::PayloadMismatch { .. })
        ),
        "the same key with a changed payload must conflict"
    );
    Ok(())
}

#[tokio::test]
async fn idempotency_key_binds_principal_scope_and_payload_hash()
-> Result<(), Box<dyn std::error::Error>> {
    let (_epistemic, bounded) = bounded_harness("bounded-binding").await?;
    let at = Utc::now();

    let mut original = transition(None, None, "key-binding", None, None);
    original.current = Some(new_belief(
        "matthew",
        "preferred_editor",
        "zed",
        scope("workshop"),
        at,
    ));
    bounded
        .apply_idempotent_transition(original.clone())
        .await?;

    // The key is qualified by principal and scope, so the same text under another
    // principal or scope is a distinct write and can never replay this receipt.
    // Decided LK-HOST-001 v1.1: global-key uniqueness would make a conflict an
    // existence oracle across principals, so binding (not contention) is correct.
    let mut foreign_principal = transition(None, None, "key-binding", None, None);
    foreign_principal.principal_id = "muse".to_owned();
    foreign_principal.current = Some(new_belief(
        "matthew",
        "preferred_editor",
        "muse-archive",
        scope("workshop"),
        at,
    ));
    let foreign_receipt = bounded
        .apply_idempotent_transition(foreign_principal)
        .await?;
    assert_ne!(
        foreign_receipt.receipt_id,
        original.receipt_id(),
        "the same key text under another principal must not share a receipt"
    );
    assert_eq!(
        foreign_receipt.outcome,
        WriteOutcome::Accepted { revision_seq: 1 },
        "another principal's write is not a replay of this one"
    );

    let mut foreign_scope = transition(None, None, "key-binding", None, None);
    foreign_scope.scope = scope("hosted-memory");
    foreign_scope.current = Some(new_belief(
        "matthew",
        "preferred_editor",
        "scoped-archive",
        scope("hosted-memory"),
        at,
    ));
    let scoped_receipt = bounded.apply_idempotent_transition(foreign_scope).await?;
    assert_ne!(
        scoped_receipt.receipt_id,
        original.receipt_id(),
        "the same key text under another scope must not share a receipt"
    );

    let mut blank_key = transition(None, None, "   ", None, None);
    blank_key.current = Some(new_belief(
        "matthew",
        "preferred_editor",
        "blank",
        scope("workshop"),
        at,
    ));
    assert!(
        matches!(
            bounded.apply_idempotent_transition(blank_key).await,
            Err(BoundedError::InvalidIdempotencyKey)
        ),
        "a blank key must be refused before any storage work"
    );

    let mut other_key = transition(None, None, "key-binding-2", None, None);
    other_key.current = Some(new_belief(
        "matthew",
        "preferred_editor",
        "other",
        scope("workshop"),
        at,
    ));
    let accepted = bounded.apply_idempotent_transition(other_key).await?;
    assert_ne!(
        accepted.receipt_id,
        original.receipt_id(),
        "a different key must produce a different receipt"
    );
    Ok(())
}
#[tokio::test]
async fn stale_revision_is_refused_without_partial_state() -> Result<(), Box<dyn std::error::Error>>
{
    let (epistemic, bounded) = bounded_harness("bounded-cas").await?;
    let at = Utc::now();
    let belief = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);

    let mut create = transition(None, None, "cas-create", None, None);
    create.current = Some(belief.clone());
    bounded.apply_idempotent_transition(create).await?;

    let mut supersede = transition(None, Some(&belief), "cas-supersede", Some(0), Some("helix"));
    supersede.current = Some({
        let mut next = belief.clone();
        next.current_value = "helix".to_owned();
        next.updated_at = Utc::now();
        next
    });
    let refused = bounded.apply_idempotent_transition(supersede).await;
    assert_eq!(
        refused.expect_err("a stale expected_revision must be refused"),
        BoundedError::RevisionConflict {
            expected: Some(0),
            actual: Some(1)
        }
    );

    assert_eq!(
        epistemic.get_belief(&belief.id).await?,
        Some(belief.clone()),
        "a refused transition must not change the projection"
    );
    assert_eq!(
        epistemic.list_belief_revisions(&belief.id).await?.len(),
        0,
        "a refused transition must not leave a revision"
    );
    assert_eq!(
        bounded.current_revision(&belief.id).await?,
        Some(1),
        "the refused write must not advance the revision"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_idempotent_transitions_commit_exactly_one()
-> Result<(), Box<dyn std::error::Error>> {
    let (epistemic, bounded) = bounded_harness("bounded-concurrent").await?;
    let at = Utc::now();
    let belief = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);

    let mut create = transition(None, None, "race-create", None, None);
    create.current = Some(belief.clone());
    bounded.apply_idempotent_transition(create).await?;

    let mut left = transition(None, Some(&belief), "race-left", Some(1), Some("helix"));
    left.current = Some({
        let mut next = belief.clone();
        next.current_value = "helix".to_owned();
        next.updated_at = Utc::now();
        next
    });
    let mut right = transition(None, Some(&belief), "race-right", Some(1), Some("vim"));
    right.current = Some({
        let mut next = belief.clone();
        next.current_value = "vim".to_owned();
        next.updated_at = Utc::now();
        next
    });

    let (left_result, right_result) = tokio::join!(
        bounded.apply_idempotent_transition(left),
        bounded.apply_idempotent_transition(right),
    );

    let accepted = [&left_result, &right_result]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(
        accepted, 1,
        "exactly one concurrent writer may commit: left={left_result:?} right={right_result:?}"
    );
    assert!(
        matches!(
            left_result.as_ref().err().or(right_result.as_ref().err()),
            Some(BoundedError::RevisionConflict { .. })
        ),
        "the losing writer must be refused with a typed conflict, not silently dropped"
    );

    let revisions = epistemic.list_belief_revisions(&belief.id).await?;
    assert_eq!(
        revisions.len(),
        1,
        "a refused concurrent write must leave no revision and no lost evidence"
    );
    assert_eq!(
        bounded.current_revision(&belief.id).await?,
        Some(2),
        "the accepted writer must advance the revision exactly once"
    );
    Ok(())
}

#[tokio::test]
async fn rejected_statement_surfaces_as_failure_not_success()
-> Result<(), Box<dyn std::error::Error>> {
    let (epistemic, bounded) = bounded_harness("bounded-rejected").await?;
    let at = Utc::now();
    let belief = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);

    let mut create = transition(None, None, "reject-create", None, None);
    create.current = Some(belief.clone());
    bounded.apply_idempotent_transition(create).await?;

    let mut accepted = transition(
        None,
        Some(&belief),
        "reject-supersede",
        Some(1),
        Some("helix"),
    );
    accepted.current = Some({
        let mut next = belief.clone();
        next.current_value = "helix".to_owned();
        next.updated_at = Utc::now();
        next
    });
    let receipt = bounded
        .apply_idempotent_transition(accepted.clone())
        .await?;
    assert_eq!(receipt.outcome, WriteOutcome::Accepted { revision_seq: 2 });
    let existing_revision = epistemic.list_belief_revisions(&belief.id).await?[0]
        .id
        .clone();

    // Inject a mid-batch failure: the revision id already exists.
    let mut failing = transition(
        None,
        accepted.current.as_ref(),
        "reject-failing",
        Some(2),
        Some("vim"),
    );
    failing.current = Some({
        let mut next = accepted.current.clone().expect("current");
        next.current_value = "vim".to_owned();
        next.updated_at = Utc::now();
        next
    });
    let failing_trace = trace("bounded.failing");
    failing.revisions = vec![lighting_core::BeliefRevision {
        id: existing_revision,
        created_at: Utc::now(),
        ..revision(&belief.id, Some("helix"), "vim", &failing_trace.id)
    }];
    failing.trace = failing_trace;

    let refused = bounded.apply_idempotent_transition(failing).await;
    assert!(
        matches!(
            refused,
            Err(BoundedError::StatementRejected { .. })
                | Err(BoundedError::RevisionConflict { .. })
        ),
        "a rolled-back batch must be reported as a typed failure, got {refused:?}"
    );

    let current = epistemic
        .get_belief(&belief.id)
        .await?
        .expect("belief must survive a rejected batch");
    assert_eq!(
        current.current_value, "helix",
        "a rejected batch must not partially apply its projection update"
    );
    assert_eq!(
        epistemic.list_belief_revisions(&belief.id).await?.len(),
        1,
        "a rejected batch must leave no orphan revision"
    );
    assert_eq!(
        bounded.current_revision(&belief.id).await?,
        Some(2),
        "a rejected batch must not advance the revision"
    );
    Ok(())
}

#[tokio::test]
async fn dependency_traversal_is_bounded_and_fails_closed() -> Result<(), Box<dyn std::error::Error>>
{
    let (epistemic, bounded) = bounded_harness("bounded-traversal").await?;
    let at = Utc::now();

    let root = new_belief("matthew", "preferred_editor", "zed", scope("workshop"), at);
    epistemic.store_belief(root.clone()).await?;

    let mut middle = new_belief(
        "matthew",
        "editor_theme",
        "solarized",
        scope("workshop"),
        at,
    );
    middle.lineage.prior_belief_id = Some(root.id.to_string());
    epistemic.store_belief(middle.clone()).await?;

    let mut leaf = new_belief("matthew", "editor_font", "jetbrains", scope("workshop"), at);
    leaf.lineage.prior_belief_id = Some(middle.id.to_string());
    epistemic.store_belief(leaf.clone()).await?;

    let overflow = bounded
        .dependency_closure(&leaf.id, TraversalBounds::new(1, MAX_TRAVERSAL_FANOUT)?)
        .await;
    assert!(
        matches!(overflow, Err(BoundedError::TraversalOverflow { .. })),
        "a traversal that would exceed the depth bound must be refused, got {overflow:?}"
    );

    let closure = bounded
        .dependency_closure(&leaf.id, TraversalBounds::new(2, MAX_TRAVERSAL_FANOUT)?)
        .await?;
    assert_eq!(
        closure.beliefs().len(),
        3,
        "a bounded traversal within the bound must return the whole chain"
    );
    assert_eq!(closure.reached_depth(), 2);

    assert!(
        TraversalBounds::new(MAX_TRAVERSAL_DEPTH + 1, MAX_TRAVERSAL_FANOUT).is_err(),
        "a depth bound above the contract maximum must be refused"
    );
    Ok(())
}
