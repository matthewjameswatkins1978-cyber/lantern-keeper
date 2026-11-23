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
    Belief, BeliefId, BeliefRevision, BeliefRevisionId, Claim, ClaimId, EpistemicRepository,
    LedgerEvent, LedgerEventRepository, LedgerIngestResult, LedgerRole, NewBelief, NewClaim,
    PredicateStatus, ReconciliationAction, Scope, Stance, Trace, TraceId, TrustClass, scope_hash,
};
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealLedgerRepository, SurrealStore,
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
#[ignore = "measured native defect at ac619cca: apply_belief_transition does not inspect \
            per-statement results, so a BEGIN/COMMIT batch that rolls back still returns \
            Ok(current). A caller records an accepted write that does not exist. Needs \
            statement-error propagation before a hosted adapter can claim durable writes."]
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
#[ignore = "measured native defect at ac619cca: two concurrent apply_belief_transition calls can \
            both return Ok while only one revision and one projection value persist, so the \
            loser is told its correction was accepted although no evidence exists. There is no \
            compare-and-set / expected-version precondition. This is the measured justification \
            for a D1 CAS requirement and for a bounded single-writer path."]
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
#[ignore = "gap: epistemic writes carry no caller-supplied idempotency key; only Claim.dedupe_key \
            (content-derived) and ledger idempotency_key provide replay keys, so an accepted \
            projection transition cannot be safely retried after a lost response"]
async fn projection_transition_replay_is_keyed_by_caller() -> Result<(), Box<dyn std::error::Error>>
{
    // A retry of apply_belief_transition after a lost response creates the trace
    // and revisions again, because revision ids are generated by the caller and
    // there is no receipt key to deduplicate on.
    Ok(())
}

#[tokio::test]
#[ignore = "gap: list_claims/list_beliefs/list_relations expose no limit or cursor, so a hosted \
            adapter must either violate the trait or invent a service-side bound"]
async fn list_calls_are_bounded_for_hosted_row_limits() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

// ---------------------------------------------------------------------------
// 8. v1.1 contract shape — recorded ahead of implementation
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "contract shape (LK-HOST-001 v1.1): no idempotency key exists in the registry, so an \
            accepted write cannot yet bind principal + scope + canonical payload hash. Exact \
            retries must recover the original receipt; a changed envelope under the same key must \
            conflict rather than create a second record."]
async fn idempotency_key_binds_principal_scope_and_payload_hash()
-> Result<(), Box<dyn std::error::Error>> {
    // Required semantics for the hosted seam, recorded before implementation:
    //   key = (authenticated principal_id, authorised scope, canonical payload hash)
    //   - identical envelope: return the stored receipt, no new record;
    //   - same key, different payload: conflict, never overwrite;
    //   - same payload, different principal or scope: distinct record, never shared.
    // Claim.dedupe_key covers content and scope but not principal, and is not a
    // receipt key. Ledger idempotency_key is per-source and not scoped.
    Ok(())
}

#[tokio::test]
#[ignore = "contract shape (LK-HOST-001 v1.1): stale/dependency traversal is unbounded in the \
            current seam (stale_beliefs is caller-supplied and no depth or fanout is capped), so \
            bounded traversal with typed nonacceptance on overflow cannot be expressed yet."]
async fn dependency_traversal_is_bounded_and_fails_closed() -> Result<(), Box<dyn std::error::Error>>
{
    // Required semantics:
    //   - traversal depth and fanout are explicit, bounded parameters;
    //   - exceeding a bound returns a typed nonacceptance, not a partial result;
    //   - nonacceptance leaves no partial commit and no orphan revision;
    //   - callers can distinguish overflow from success and from invalid input.
    Ok(())
}
