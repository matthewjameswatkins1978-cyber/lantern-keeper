//! Backend-neutral memory-contract fixtures (LK-Q10).
//!
//! The same clauses must hold for every adapter that carries Lantern memory. This
//! file makes that executable and honest in one place:
//!
//! * every clause is declared once (`Clause`);
//! * every adapter is declared once (`Adapter`);
//! * every (adapter, clause) cell is either `Executed` — and the matrix test runs
//!   the real assertion in this process — or `Pending` with a reason that names
//!   what is missing and who owns it.
//!
//! A cell that cannot run is never reported as a pass. The matrix test refuses an
//! empty reason, refuses a duplicate cell, and refuses a clause with no cell for
//! an adapter, so "we forgot to check" and "it quietly passed" are both errors.
//!
//! Native adapter: the real embedded SurrealKV repositories. Hosted adapter:
//! `lighting-store-d1` on this lineage exposes schema, statement builders and the
//! keyed correction batch, but no capture/get path and no executor, so its cells
//! are `Pending` by construction rather than skipped by convenience.

use std::collections::BTreeMap;

use chrono::Utc;
use lighting_core::{
    Belief, BeliefId, BeliefRevision, BeliefRevisionId, Claim, EpistemicRepository, MemoryItem,
    MemoryItemKind, MemoryItemSearch, NewBelief, NewClaim, NewMemoryItem, PredicateStatus, Scope,
    Source, SourceContent, SourceKind, SourceRepository, SourceTitle, Stance, Trace, TraceId,
    TrustClass,
};
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

const HOSTED_GAP: &str = "lighting-store-d1 on this lineage has schema, statement builders and the keyed correction \
     batch only: no capture/get path and no runtime executor. Owned by C2/C3, not faked here.";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Adapter {
    Native,
    HostedD1,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Clause {
    ExactSourceBytes,
    Utf8RoundTrip,
    CrlfPreservation,
    AttributedCapture,
    NoteIsNotAClaim,
    ValueHistory,
    StaleSuppression,
    ReplayIdempotent,
    NoFalseSuccess,
}

const CLAUSES: &[Clause] = &[
    Clause::ExactSourceBytes,
    Clause::Utf8RoundTrip,
    Clause::CrlfPreservation,
    Clause::AttributedCapture,
    Clause::NoteIsNotAClaim,
    Clause::ValueHistory,
    Clause::StaleSuppression,
    Clause::ReplayIdempotent,
    Clause::NoFalseSuccess,
];

const ADAPTERS: &[Adapter] = &[Adapter::Native, Adapter::HostedD1];

enum Cell {
    Executed,
    Pending(&'static str),
}

fn matrix() -> Vec<((Adapter, Clause), Cell)> {
    let mut cells = Vec::new();
    for adapter in ADAPTERS {
        for clause in CLAUSES {
            let cell = match (adapter, clause) {
                (Adapter::Native, Clause::NoFalseSuccess) => Cell::Pending(
                    "this lineage predates the repair: a rolled-back batch still returns Ok \
                     (measured on the M1 bounded surface). Fixed on \
                     fix/native-write-failure-report @ c57907f; asserted there, not here.",
                ),
                (Adapter::Native, _) => Cell::Executed,
                (Adapter::HostedD1, _) => Cell::Pending(HOSTED_GAP),
            };
            cells.push(((*adapter, *clause), cell));
        }
    }
    cells
}

struct Harness {
    epistemic: SurrealEpistemicRepository,
    sources: SurrealSourceRepository,
}

async fn harness(label: &str) -> Result<Harness, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "lantern-contract-{label}-{}",
        Uuid::new_v4().simple()
    ));
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "memory_contract".to_owned();
    config.database = "fixtures".to_owned();
    config.username.clear();
    config.password.clear();

    let store = SurrealStore::connect(&config).await?;
    store.initialise_schema().await?;
    let epistemic = SurrealEpistemicRepository::new(store.clone());
    epistemic.migrate().await?;
    let sources = SurrealSourceRepository::new(store);
    sources.migrate().await?;
    Ok(Harness { epistemic, sources })
}

fn scope() -> Scope {
    BTreeMap::from([("project".to_owned(), "contract".to_owned())])
}

fn claim(value: &str, speaker: &str) -> Claim {
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
        speaker_actor_id: speaker.to_owned(),
        transmitter_actor_id: Some("lucy".to_owned()),
        holder_actor_id: Some("matthew".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: at,
        valid_from: Some(at),
        valid_to: None,
        extractor: "contract-fixture".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: at,
    })
    .expect("claim fixture")
}

fn trace() -> Trace {
    Trace {
        id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        event_type: "contract.fixture".to_owned(),
        actor_id: "matthew".to_owned(),
        subject_id: None,
        input_ids: Vec::new(),
        output_ids: Vec::new(),
        details: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn revision(belief_id: &BeliefId, from: &str, to: &str) -> BeliefRevision {
    BeliefRevision {
        id: BeliefRevisionId::new(Uuid::new_v4().to_string()).expect("revision id"),
        belief_id: belief_id.clone(),
        previous_value: Some(from.to_owned()),
        new_value: Some(to.to_owned()),
        claim_id: None,
        transition_type: lighting_core::ReconciliationAction::Supersede,
        trace_id: TraceId::new(Uuid::new_v4().to_string()).expect("trace id"),
        created_at: Utc::now(),
    }
}

/// Exact bytes, multi-byte UTF-8 and CRLF must survive storage unchanged.
async fn exact_source_bytes(harness: &Harness) -> Result<(), String> {
    let text = "line one\r\nline two — café 🏮\r\n";
    let source = Source::create(lighting_core::NewSource {
        kind: SourceKind::PlainText,
        title: SourceTitle::new("contract-bytes".to_owned()).map_err(debug)?,
        content: SourceContent::new(text.to_owned()).map_err(debug)?,
    });
    harness.sources.store(source.clone()).await.map_err(debug)?;
    let read = harness
        .sources
        .get(source.id())
        .await
        .map_err(debug)?
        .ok_or_else(|| "stored source was not readable".to_owned())?;
    if read.content().as_bytes() != text.as_bytes() {
        return Err("source bytes changed in storage".to_owned());
    }
    if read.content().as_bytes() != source.content().as_bytes() {
        return Err("read-back bytes differ from the captured bytes".to_owned());
    }
    Ok(())
}

/// The multi-byte characters must still be there, byte-for-byte.
async fn utf8_round_trip(harness: &Harness) -> Result<(), String> {
    let text = "café 🏮 Ω";
    let source = Source::create(lighting_core::NewSource {
        kind: SourceKind::PlainText,
        title: SourceTitle::new("contract-utf8".to_owned()).map_err(debug)?,
        content: SourceContent::new(text.to_owned()).map_err(debug)?,
    });
    harness.sources.store(source.clone()).await.map_err(debug)?;
    let read = harness
        .sources
        .get(source.id())
        .await
        .map_err(debug)?
        .ok_or_else(|| "stored source was not readable".to_owned())?;
    if read.content().as_str() != text {
        return Err(format!(
            "utf8 round-trip changed the text: {:?}",
            read.content().as_str()
        ));
    }
    Ok(())
}

/// CRLF must not be normalised to LF anywhere in the pipeline.
async fn crlf_preservation(harness: &Harness) -> Result<(), String> {
    let text = "a\r\nb\r\nc";
    let source = Source::create(lighting_core::NewSource {
        kind: SourceKind::PlainText,
        title: SourceTitle::new("contract-crlf".to_owned()).map_err(debug)?,
        content: SourceContent::new(text.to_owned()).map_err(debug)?,
    });
    harness.sources.store(source.clone()).await.map_err(debug)?;
    let read = harness
        .sources
        .get(source.id())
        .await
        .map_err(debug)?
        .ok_or_else(|| "stored source was not readable".to_owned())?;
    let carriage_returns = read
        .content()
        .as_bytes()
        .iter()
        .filter(|b| **b == b'\r')
        .count();
    if carriage_returns != 2 {
        return Err(format!(
            "expected 2 carriage returns, found {carriage_returns}"
        ));
    }
    Ok(())
}

/// Attribution is preserved: who said it, who passed it on, who holds it.
async fn attributed_capture(harness: &Harness) -> Result<(), String> {
    let claim = claim("zed", "lucy");
    harness
        .epistemic
        .store_claim(claim.clone())
        .await
        .map_err(debug)?;
    let read = harness
        .epistemic
        .get_claim(&claim.id)
        .await
        .map_err(debug)?
        .ok_or_else(|| "stored claim was not readable".to_owned())?;
    if read.speaker_actor_id != "lucy"
        || read.originator_actor_id != "matthew"
        || read.transmitter_actor_id.as_deref() != Some("lucy")
        || read.holder_actor_id.as_deref() != Some("matthew")
    {
        return Err("attribution fields did not round-trip".to_owned());
    }
    if read.stance != lighting_core::Stance::Endorsing {
        return Err("stance did not round-trip".to_owned());
    }
    Ok(())
}

/// A soft note is not a claim: the same text lives in different material.
async fn note_is_not_a_claim(harness: &Harness) -> Result<(), String> {
    let text = "maybe use helix";
    let note = MemoryItem::new(NewMemoryItem {
        episode_id: None,
        source_id: None,
        kind: MemoryItemKind::Idea,
        content: text.to_owned(),
        originator_actor_id: Some("matthew".to_owned()),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        salience: 0.5,
        created_at: Utc::now(),
    })
    .map_err(debug)?;
    harness
        .epistemic
        .store_memory_item(note.clone())
        .await
        .map_err(debug)?;

    let claim = claim(text, "matthew");
    harness
        .epistemic
        .store_claim(claim.clone())
        .await
        .map_err(debug)?;

    let notes = harness
        .epistemic
        .search_memory_items(&MemoryItemSearch {
            phrase: Some(text.to_owned()),
            include_archived: false,
            limit: 10,
        })
        .await
        .map_err(debug)?;
    if notes.iter().all(|item| item.id != note.id) {
        return Err("the soft note was not found among memory items".to_owned());
    }
    let claims = harness.epistemic.list_claims(false).await.map_err(debug)?;
    if !claims.iter().any(|stored| stored.id == claim.id) {
        return Err("the claim was not found among claims".to_owned());
    }
    // The note must not appear as a claim.
    if claims
        .iter()
        .any(|stored| stored.value == text && stored.id != claim.id)
    {
        return Err("a soft note leaked into the claim collection".to_owned());
    }
    Ok(())
}

/// History keeps the old value when the projection moves to a new one.
async fn value_history(harness: &Harness) -> Result<(), String> {
    let at = Utc::now();
    let first = Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "matthew".to_owned(),
        predicate_key: "preferred_editor".to_owned(),
        current_value: "zed".to_owned(),
        scope: scope(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: at,
        valid_from: Some(at),
        created_at: at,
    })
    .map_err(debug)?;
    harness
        .epistemic
        .store_belief(first.clone())
        .await
        .map_err(debug)?;

    let mut second = first.clone();
    second.current_value = "helix".to_owned();
    second.updated_at = Utc::now();
    harness
        .epistemic
        .apply_belief_transition(
            Some(first.clone()),
            Some(second.clone()),
            vec![revision(&first.id, "zed", "helix")],
            trace(),
            Vec::new(),
        )
        .await
        .map_err(debug)?;

    let history = harness
        .epistemic
        .list_belief_revisions(&first.id)
        .await
        .map_err(debug)?;
    if history.len() != 1 {
        return Err(format!("expected 1 revision, found {}", history.len()));
    }
    if history[0].previous_value.as_deref() != Some("zed")
        || history[0].new_value.as_deref() != Some("helix")
    {
        return Err("revision did not record old and new values".to_owned());
    }
    Ok(())
}

/// Stale projections are suppressed from the default read and visible on request.
async fn stale_suppression(harness: &Harness) -> Result<(), String> {
    let at = Utc::now();
    let belief = Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "matthew".to_owned(),
        predicate_key: "editor_theme".to_owned(),
        current_value: "solarized".to_owned(),
        scope: scope(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: at,
        valid_from: Some(at),
        created_at: at,
    })
    .map_err(debug)?;
    harness
        .epistemic
        .store_belief(belief.clone())
        .await
        .map_err(debug)?;
    harness
        .epistemic
        .mark_belief_stale(&belief.id, "dependency moved", Utc::now())
        .await
        .map_err(debug)?;

    let fresh = harness.epistemic.list_beliefs(false).await.map_err(debug)?;
    let all = harness.epistemic.list_beliefs(true).await.map_err(debug)?;
    if fresh.iter().any(|stored| stored.id == belief.id) {
        return Err("a stale belief appeared in the default listing".to_owned());
    }
    if !all.iter().any(|stored| stored.id == belief.id) {
        return Err("a stale belief was not visible on explicit request".to_owned());
    }
    Ok(())
}

/// An identical capture is a replay, not a second record.
async fn replay_idempotent(harness: &Harness) -> Result<(), String> {
    let first = claim("vim", "matthew");
    let stored = harness
        .epistemic
        .store_claim(first.clone())
        .await
        .map_err(debug)?;
    let replay = harness
        .epistemic
        .store_claim(claim("vim", "matthew"))
        .await
        .map_err(debug)?;
    if replay.id != stored.id {
        return Err("a content replay produced a new claim identity".to_owned());
    }
    let all = harness.epistemic.list_claims(false).await.map_err(debug)?;
    let same_value = all.iter().filter(|stored| stored.value == "vim").count();
    if same_value != 1 {
        return Err(format!(
            "a replay duplicated the claim: {same_value} records for one value"
        ));
    }
    Ok(())
}

fn debug(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

/// Every cell accounted for, nothing faked, and the native cells actually run.
#[tokio::test]
async fn contract_matrix_is_complete_and_native_clauses_execute()
-> Result<(), Box<dyn std::error::Error>> {
    let cells = matrix();

    // Coverage: exactly one cell per (adapter, clause).
    let mut seen: BTreeMap<(Adapter, Clause), usize> = BTreeMap::new();
    for (key, _) in &cells {
        *seen.entry(*key).or_default() += 1;
    }
    for adapter in ADAPTERS {
        for clause in CLAUSES {
            let count = seen.get(&(*adapter, *clause)).copied().unwrap_or(0);
            assert_eq!(
                count, 1,
                "clause {clause:?} for adapter {adapter:?} must have exactly one cell, found {count}"
            );
        }
    }
    assert_eq!(cells.len(), ADAPTERS.len() * CLAUSES.len());

    // A pending cell must say why; an executed cell must actually run below.
    for ((adapter, clause), cell) in &cells {
        if let Cell::Pending(reason) = cell {
            assert!(
                reason.len() > 40,
                "pending reason for {adapter:?}/{clause:?} is too thin to be useful: {reason:?}"
            );
        }
    }

    let harness = harness("matrix").await?;
    let mut executed = 0usize;
    for ((adapter, clause), cell) in &cells {
        if *adapter != Adapter::Native {
            continue;
        }
        if !matches!(cell, Cell::Executed) {
            continue;
        }
        let outcome = match clause {
            Clause::ExactSourceBytes => exact_source_bytes(&harness).await,
            Clause::Utf8RoundTrip => utf8_round_trip(&harness).await,
            Clause::CrlfPreservation => crlf_preservation(&harness).await,
            Clause::AttributedCapture => attributed_capture(&harness).await,
            Clause::NoteIsNotAClaim => note_is_not_a_claim(&harness).await,
            Clause::ValueHistory => value_history(&harness).await,
            Clause::StaleSuppression => stale_suppression(&harness).await,
            Clause::ReplayIdempotent => replay_idempotent(&harness).await,
            Clause::NoFalseSuccess => unreachable!("declared Pending"),
        };
        outcome.unwrap_or_else(|error| panic!("native clause {clause:?} failed: {error}"));
        executed += 1;
    }
    assert_eq!(
        executed, 8,
        "eight native clauses must execute; a cell that runs nothing is not a pass"
    );

    // Hosted cells stay pending: prove the matrix says so rather than skipping them.
    let hosted_pending = cells
        .iter()
        .filter(|((adapter, _), cell)| {
            *adapter == Adapter::HostedD1 && matches!(cell, Cell::Pending(_))
        })
        .count();
    assert_eq!(
        hosted_pending,
        CLAUSES.len(),
        "every hosted cell must be explicitly pending on this lineage"
    );
    Ok(())
}

/// Focused runs: one test per executed native clause.
#[tokio::test]
async fn native_exact_source_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("bytes").await?;
    exact_source_bytes(&harness)
        .await
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[tokio::test]
async fn native_utf8_and_crlf_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("utf8").await?;
    utf8_round_trip(&harness)
        .await
        .map_err(std::io::Error::other)?;
    crlf_preservation(&harness)
        .await
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[tokio::test]
async fn native_attribution_and_history() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("history").await?;
    attributed_capture(&harness)
        .await
        .map_err(std::io::Error::other)?;
    value_history(&harness)
        .await
        .map_err(std::io::Error::other)?;
    Ok(())
}

#[tokio::test]
async fn native_stale_and_replay() -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness("stale").await?;
    stale_suppression(&harness)
        .await
        .map_err(std::io::Error::other)?;
    replay_idempotent(&harness)
        .await
        .map_err(std::io::Error::other)?;
    Ok(())
}
