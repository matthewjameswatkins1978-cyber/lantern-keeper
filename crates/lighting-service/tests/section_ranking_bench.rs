//! LK-Q03 — labeled section-ranking corpus and honest baseline metrics.
//!
//! Loads `fixtures/section-ranking/corpus.json` (synthetic, generated — no
//! private workshop content), seeds an isolated embedded store, and evaluates
//! every labeled query across the four baseline families:
//!
//! * `exact_identifier` — exact identifiers must surface their record;
//! * `current_vs_superseded` — current projections surface, superseded stay
//!   out of ordinary queries, and appear under explicit history queries;
//! * `structured_heading` — outline extraction returns the labeled headings
//!   and skips fenced pseudo-headings;
//! * `scope_and_budget` — exact-scope retrieval excludes stronger cross-scope
//!   decoys; item/token budgets are honoured.
//!
//! The runner is a BASELINE, not a gate: per-family hit rates and item-level
//! recall are printed as metrics (`--nocapture`), and misses are listed
//! honestly. Structural guarantees DO fail the run: budgets must never be
//! exceeded, every query must be evaluated against a known mode, and every
//! label reference must resolve.
//!
//! A second test seeds one claim with lineage so an `exact_identifier` pack
//! carries non-empty `source_refs` and bounded section hits. Fixture/test-only:
//! the 32-query corpus and the baseline assertions above are untouched.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::Deserialize;

use lighting_core::{
    Belief, BeliefLineage, BeliefState, Claim, EpistemicRepository, NewBelief, NewClaim, NewSource,
    PredicateStatus, Source, SourceContent, SourceKind, SourceRepository, SourceTitle, Stance,
    StoreSourceResult, TrustClass,
};
use lighting_service::epistemic_dto::ContextCompileRequest;
use lighting_service::{EpistemicService, source_ops::SourceService};
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealMemoryPathRepository, SurrealSourceRepository,
    SurrealStore,
};
use uuid::Uuid;

const CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/section-ranking/corpus.json"
);
const EXPECTED_QUERY_COUNT: usize = 32;

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    notes: Vec<CorpusNote>,
    beliefs: Vec<CorpusBelief>,
    queries: Vec<CorpusQuery>,
}

/// Note-level heading labels in the corpus are documentation for fixture
/// readers; queries carry the labels the runner enforces.
#[derive(Debug, Deserialize)]
struct CorpusNote {
    title: String,
    kind: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct CorpusBelief {
    #[serde(rename = "ref")]
    reference: String,
    subject_key: String,
    predicate_key: String,
    current_value: String,
    scope: BTreeMap<String, String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CorpusQuery {
    id: String,
    family: String,
    mode: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    scope: Option<BTreeMap<String, String>>,
    #[serde(default)]
    item_budget: Option<usize>,
    #[serde(default)]
    token_budget: Option<usize>,
    #[serde(default)]
    expect_present: Vec<String>,
    #[serde(default)]
    expect_absent: Vec<String>,
    #[serde(default)]
    expect_headings: Vec<(u8, String)>,
    #[serde(default)]
    expect_headings_absent: Vec<(u8, String)>,
}

#[derive(Debug, Default, Clone)]
struct FamilyStats {
    queries: usize,
    query_hits: usize,
    items_expected: usize,
    items_hit: usize,
}

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-q03-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "m1_section_ranking".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

#[tokio::test]
async fn section_ranking_corpus_baseline() -> Result<(), Box<dyn Error>> {
    let raw = fs::read_to_string(CORPUS_PATH)
        .map_err(|error| format!("cannot read corpus at {CORPUS_PATH}: {error}"))?;
    let corpus: Corpus = serde_json::from_str(&raw)?;
    assert_eq!(
        corpus.schema, "lantern-section-ranking-corpus-v1",
        "unexpected corpus schema"
    );
    assert_eq!(
        corpus.queries.len(),
        EXPECTED_QUERY_COUNT,
        "corpus must carry the full labeled query set"
    );

    let store = SurrealStore::connect(&embedded_config(test_path("baseline"))).await?;
    store.initialise_schema().await?;
    let epistemic_repo = Arc::new(SurrealEpistemicRepository::new(store.clone()));
    epistemic_repo.migrate().await?;
    let source_repo = Arc::new(SurrealSourceRepository::new(store.clone()));
    source_repo.migrate().await?;

    let epistemic = EpistemicService::new(epistemic_repo.clone());
    let sources = SourceService::new(source_repo.clone());

    // Seed notes (structured Markdown, heading labels live in the corpus).
    for note in &corpus.notes {
        let kind = if note.kind == "markdown" {
            SourceKind::Markdown
        } else {
            SourceKind::PlainText
        };
        let source = Source::create(NewSource {
            kind,
            title: SourceTitle::new(note.title.clone())
                .map_err(|error| format!("bad note title: {error}"))?,
            content: SourceContent::new(note.content.clone())
                .map_err(|error| format!("bad note content: {error}"))?,
        });
        source_repo.store(source).await?;
    }

    // Seed beliefs with staggered clocks so tie-breaks are deterministic
    // (sort order: score desc, updated_at desc, id asc).
    let base = Utc::now();
    let mut ref_to_id: BTreeMap<String, String> = BTreeMap::new();
    for (index, entry) in corpus.beliefs.iter().enumerate() {
        let at = base + Duration::milliseconds(index as i64);
        let mut belief = Belief::new(NewBelief {
            holder_key: "matthew".to_owned(),
            subject_key: entry.subject_key.clone(),
            predicate_key: entry.predicate_key.clone(),
            current_value: entry.current_value.clone(),
            scope: entry.scope.clone(),
            confidence: 0.9,
            trust_class: TrustClass::Direct,
            known_from: at,
            valid_from: Some(at),
            created_at: at,
        })
        .map_err(|error| format!("belief {} failed to construct: {error}", entry.reference))?;
        belief.updated_at = at;
        if entry.state.as_deref() == Some("Superseded") {
            belief.state = BeliefState::Superseded;
        }
        let stored = epistemic_repo.store_belief(belief).await?;
        ref_to_id.insert(entry.reference.clone(), stored.id.to_string());
    }

    let mut stats: BTreeMap<String, FamilyStats> = BTreeMap::new();
    let mut misses: Vec<String> = Vec::new();
    let mut budget_violations: Vec<String> = Vec::new();
    let mut evaluated = 0usize;

    for query in &corpus.queries {
        let family = stats.entry(query.family.clone()).or_default();
        family.queries += 1;
        let mut query_hit = true;
        let mut items_expected = 0usize;
        let mut items_hit = 0usize;

        match query.mode.as_str() {
            "outline" => {
                let note = query
                    .note
                    .as_deref()
                    .ok_or_else(|| format!("{}: outline query needs a note", query.id))?;
                let outline = sources
                    .get_outline(SourceKind::Markdown, note)
                    .await?
                    .ok_or_else(|| format!("{}: note {note:?} has no outline", query.id))?;
                for (level, title) in &query.expect_headings {
                    items_expected += 1;
                    if outline
                        .headings
                        .iter()
                        .any(|heading| heading.level == *level && &heading.title == title)
                    {
                        items_hit += 1;
                    } else {
                        query_hit = false;
                        misses.push(format!(
                            "{}: heading ({level}, {title:?}) not in outline",
                            query.id
                        ));
                    }
                }
                for (level, title) in &query.expect_headings_absent {
                    if outline
                        .headings
                        .iter()
                        .any(|heading| heading.level == *level && &heading.title == title)
                    {
                        query_hit = false;
                        misses.push(format!(
                            "{}: fenced pseudo-heading ({level}, {title:?}) was extracted",
                            query.id
                        ));
                    }
                }
            }
            "context" | "scoped_context" => {
                let text = query
                    .query
                    .as_deref()
                    .ok_or_else(|| format!("{}: context query needs text", query.id))?;
                let request = ContextCompileRequest {
                    query: text.to_owned(),
                    actor: Some("bench".to_owned()),
                    project_hints: Vec::new(),
                    scope: query.scope.clone().unwrap_or_default(),
                    intent: None,
                    item_budget: query.item_budget.unwrap_or(10),
                    token_budget: query.token_budget.unwrap_or(2048),
                };
                let pack = if query.mode == "context" {
                    epistemic.compile_context(request).await?
                } else {
                    epistemic.compile_context_scoped(request).await?
                };

                // Budgets are hard guarantees, not metrics.
                let trace = &pack.retrieval_trace;
                if trace.estimated_token_usage > trace.token_budget {
                    budget_violations.push(format!(
                        "{}: estimated {} tokens exceeds budget {}",
                        query.id, trace.estimated_token_usage, trace.token_budget
                    ));
                }
                if pack.selected_ids.len() > trace.item_budget {
                    budget_violations.push(format!(
                        "{}: selected {} items exceeds budget {}",
                        query.id,
                        pack.selected_ids.len(),
                        trace.item_budget
                    ));
                }

                for reference in &query.expect_present {
                    items_expected += 1;
                    let id = ref_to_id
                        .get(reference)
                        .ok_or_else(|| format!("{}: unknown reference {reference}", query.id))?;
                    if pack.selected_ids.iter().any(|selected| selected == id) {
                        items_hit += 1;
                    } else {
                        query_hit = false;
                        misses.push(format!(
                            "{}: expected record {reference} not selected (selected {} items)",
                            query.id,
                            pack.selected_ids.len()
                        ));
                    }
                }
                for reference in &query.expect_absent {
                    let id = ref_to_id
                        .get(reference)
                        .ok_or_else(|| format!("{}: unknown reference {reference}", query.id))?;
                    if pack.selected_ids.iter().any(|selected| selected == id) {
                        query_hit = false;
                        misses.push(format!(
                            "{}: excluded record {reference} WAS selected",
                            query.id
                        ));
                    }
                }
            }
            other => {
                return Err(format!("{}: unknown query mode {other:?}", query.id).into());
            }
        }

        evaluated += 1;
        family.items_expected += items_expected;
        family.items_hit += items_hit;
        if query_hit {
            family.query_hits += 1;
        }
    }

    assert_eq!(
        evaluated,
        corpus.queries.len(),
        "every labeled query must be evaluated"
    );
    assert!(
        budget_violations.is_empty(),
        "budget guarantees violated: {budget_violations:?}"
    );

    // Honest baseline report (visible with --nocapture).
    let mut total = FamilyStats::default();
    println!();
    println!(
        "== LK-Q03 section-ranking baseline: {} queries, {} beliefs, {} notes ==",
        corpus.queries.len(),
        corpus.beliefs.len(),
        corpus.notes.len()
    );
    println!(
        "{:<26} {:>10} {:>16}",
        "family", "queries", "items (hit/exp)"
    );
    for (family, stats) in &stats {
        println!(
            "{:<26} {:>4} hit/{:<4} {:>6} hit/{:<6} (recall {:.0}%)",
            family,
            stats.query_hits,
            stats.queries,
            stats.items_hit,
            stats.items_expected,
            if stats.items_expected == 0 {
                100.0
            } else {
                (stats.items_hit as f64 / stats.items_expected as f64) * 100.0
            }
        );
        total.queries += stats.queries;
        total.query_hits += stats.query_hits;
        total.items_expected += stats.items_expected;
        total.items_hit += stats.items_hit;
    }
    println!(
        "{:<26} {:>4} hit/{:<4} {:>6} hit/{:<6} (recall {:.0}%)",
        "OVERALL",
        total.query_hits,
        total.queries,
        total.items_hit,
        total.items_expected,
        if total.items_expected == 0 {
            100.0
        } else {
            (total.items_hit as f64 / total.items_expected as f64) * 100.0
        }
    );
    if !misses.is_empty() {
        println!("misses ({}):", misses.len());
        for miss in &misses {
            println!("  - {miss}");
        }
    } else {
        println!("misses: none");
    }
    println!("budget violations: none (hard guarantee held)");

    // Structural floor only: every family must exist and be non-empty; the
    // baseline may honestly be below 100% until the section-ranking slice
    // lands, so recall is reported, never gated.
    for family in [
        "exact_identifier",
        "current_vs_superseded",
        "structured_heading",
        "scope_and_budget",
    ] {
        let stats = stats
            .get(family)
            .ok_or_else(|| format!("family {family} has no queries"))?;
        assert!(stats.queries > 0, "family {family} must be evaluated");
    }
    Ok(())
}

/// Claim-seeded scenario (follow-up flag): one claim with lineage attached to
/// the `exact_identifier` record, so the pack's `source_refs` is non-empty and
/// section hits attach from the claim's evidence note. Both halves run against
/// one deterministic store: a lineage-free pack documents the empty
/// `source_refs`/empty-sections state, then the lineaged query proves
/// non-empty, capped, byte-bounded sections. Fixture/test-only — the 32-query
/// corpus and the baseline test above are untouched.
#[tokio::test]
async fn section_ranking_claim_seeded_sections() -> Result<(), Box<dyn Error>> {
    const SEEDED_NOTE_TITLE: &str = "bench: registry claims";
    const SEEDED_NOTE_CONTENT: &str = "\
# Registry Claims

## Registry entry rx-1042
rx-1042 verified ready in the bench registry.

## Unrelated entries
Other rows without the target identifier.
";
    /// Mirrors `MAX_SECTION_HITS` in `lighting_service::epistemic_ops`.
    const SECTIONS_CAP: usize = 10;

    let raw = fs::read_to_string(CORPUS_PATH)
        .map_err(|error| format!("cannot read corpus at {CORPUS_PATH}: {error}"))?;
    let corpus: Corpus = serde_json::from_str(&raw)?;
    assert_eq!(
        corpus.queries.len(),
        EXPECTED_QUERY_COUNT,
        "claim-seeded test reads the same full corpus"
    );

    let store = SurrealStore::connect(&embedded_config(test_path("claim-seeded"))).await?;
    store.initialise_schema().await?;
    let epistemic_repo = Arc::new(SurrealEpistemicRepository::new(store.clone()));
    epistemic_repo.migrate().await?;
    let source_repo = Arc::new(SurrealSourceRepository::new(store.clone()));
    source_repo.migrate().await?;
    let memory_path_repo = Arc::new(SurrealMemoryPathRepository::new(store.clone()));
    memory_path_repo.migrate().await?;
    let epistemic = EpistemicService::new_with_evidence(
        epistemic_repo.clone(),
        source_repo.clone(),
        memory_path_repo.clone(),
    );

    // Corpus notes, seeded exactly as the baseline seeds them.
    for note in &corpus.notes {
        let kind = if note.kind == "markdown" {
            SourceKind::Markdown
        } else {
            SourceKind::PlainText
        };
        let source = Source::create(NewSource {
            kind,
            title: SourceTitle::new(note.title.clone())
                .map_err(|error| format!("bad note title: {error}"))?,
            content: SourceContent::new(note.content.clone())
                .map_err(|error| format!("bad note content: {error}"))?,
        });
        source_repo.store(source).await?;
    }

    // Evidence note whose outline sections carry the queried identifier.
    let seeded = match source_repo
        .store(Source::create(NewSource {
            kind: SourceKind::Markdown,
            title: SourceTitle::new(SEEDED_NOTE_TITLE)
                .map_err(|error| format!("bad seeded title: {error}"))?,
            content: SourceContent::new(SEEDED_NOTE_CONTENT)
                .map_err(|error| format!("bad seeded content: {error}"))?,
        }))
        .await?
    {
        StoreSourceResult::Stored(source) => source,
        StoreSourceResult::Duplicate { existing_id, .. } => {
            return Err(format!("seeded note collided with {}", existing_id.as_str()).into());
        }
    };
    let seeded_source_id = seeded.id().as_str().to_owned();
    let span_start = SEEDED_NOTE_CONTENT
        .find("rx-1042")
        .ok_or_else(|| "seeded note must contain the queried identifier".to_string())?;

    // One claim sourced from that note.
    let claim = Claim::new(NewClaim {
        episode_id: None,
        source_id: Some(seeded_source_id.clone()),
        evidence_span: Some((span_start, span_start + "rx-1042".len())),
        subject_key: "bench.registry".to_owned(),
        predicate_key: Some("status".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: "registry entry rx-1042 ready".to_owned(),
        scope: BTreeMap::from([("project".to_owned(), "bench".to_owned())]),
        polarity: true,
        originator_actor_id: "bench".to_owned(),
        speaker_actor_id: "bench".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: None,
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 0.9,
        known_at: Utc::now(),
        valid_from: None,
        valid_to: None,
        extractor: "bench-fixture".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: Utc::now(),
    })
    .map_err(|error| format!("claim failed to construct: {error}"))?;
    let claim = epistemic_repo.store_claim(claim).await?;

    // Seeded beliefs (staggered clocks, same as the baseline); the
    // exact-identifier record e-1 carries lineage to the claim.
    let base = Utc::now();
    let mut ref_to_id: BTreeMap<String, String> = BTreeMap::new();
    for (index, entry) in corpus.beliefs.iter().enumerate() {
        let at = base + Duration::milliseconds(index as i64);
        let mut belief = Belief::new(NewBelief {
            holder_key: "matthew".to_owned(),
            subject_key: entry.subject_key.clone(),
            predicate_key: entry.predicate_key.clone(),
            current_value: entry.current_value.clone(),
            scope: entry.scope.clone(),
            confidence: 0.9,
            trust_class: TrustClass::Direct,
            known_from: at,
            valid_from: Some(at),
            created_at: at,
        })
        .map_err(|error| format!("belief {} failed to construct: {error}", entry.reference))?;
        belief.updated_at = at;
        if entry.state.as_deref() == Some("Superseded") {
            belief.state = BeliefState::Superseded;
        }
        if entry.reference == "e-1" {
            belief.lineage = BeliefLineage {
                supporting_claim_ids: vec![claim.id.as_str().to_owned()],
                ..BeliefLineage::default()
            };
        }
        let stored = epistemic_repo.store_belief(belief).await?;
        ref_to_id.insert(entry.reference.clone(), stored.id.to_string());
    }

    let context_request = |query: &str, project: &str| ContextCompileRequest {
        query: query.to_owned(),
        actor: Some("bench".to_owned()),
        project_hints: Vec::new(),
        scope: BTreeMap::from([("project".to_owned(), project.to_owned())]),
        intent: None,
        item_budget: 8,
        token_budget: 2048,
    };

    // BEFORE — lineage-free selection: the exact-scope lane cannot see e-1's
    // claim, so source_refs and sections stay empty (the pre-seed state).
    let before = epistemic
        .compile_context_scoped(context_request("sapphire valve v1 open", "bench-scope"))
        .await?;
    let s1 = ref_to_id
        .get("s-1")
        .ok_or_else(|| "missing s-1".to_string())?;
    assert!(
        before.selected_ids.iter().any(|selected| selected == s1),
        "before: the scoped family query must select s-1"
    );
    assert!(
        before.source_refs.is_empty(),
        "before: no lineage in the selection -> empty source_refs"
    );
    assert!(
        before.sections.is_empty(),
        "before: no evidence sources -> no section hits"
    );

    // AFTER — e-1 carries lineage to the claim: source_refs fills from the
    // claim's evidence note and bounded sections attach for the query.
    let after = epistemic
        .compile_context(context_request("rx-1042", "bench"))
        .await?;
    let e1 = ref_to_id
        .get("e-1")
        .ok_or_else(|| "missing e-1".to_string())?;
    assert!(
        after.selected_ids.iter().any(|selected| selected == e1),
        "after: the exact-identifier query must still select e-1"
    );
    assert_eq!(
        after.source_refs,
        vec![seeded_source_id.clone()],
        "after: the claim's lineage surfaces exactly the evidence note"
    );
    assert!(
        !after.sections.is_empty(),
        "after: sections must appear for the claim-seeded query"
    );
    assert!(
        after.sections.len() <= SECTIONS_CAP,
        "after: section hits must stay within the cap ({} > {SECTIONS_CAP})",
        after.sections.len()
    );
    let stored_content = seeded.content().as_str();
    for section in &after.sections {
        assert_eq!(
            section.source_id, seeded_source_id,
            "after: every section must come from the claim's evidence note"
        );
        assert!(
            section.score > 0,
            "after: attached sections must overlap the query"
        );
        assert!(
            section.start_byte < section.end_byte && section.end_byte <= stored_content.len(),
            "after: section byte ranges must stay inside the note"
        );
    }
    assert!(
        after
            .sections
            .iter()
            .any(|section| section.title.contains("rx-1042")),
        "after: the identifier heading must rank among the section hits"
    );
    assert!(
        after
            .retrieval_trace
            .lanes
            .iter()
            .any(|lane| lane == "section-overlap"),
        "after: the section-overlap lane must be recorded"
    );
    assert!(
        after.retrieval_trace.estimated_token_usage <= after.retrieval_trace.token_budget,
        "after: estimated tokens must stay within the budget"
    );
    assert!(
        after.selected_ids.len() <= after.retrieval_trace.item_budget,
        "after: selection must stay within the item budget"
    );

    // Before/after receipt (visible with --nocapture).
    println!();
    println!("== claim-seeded section scenario ==");
    println!(
        "{:<6} {:<24} {:>11} {:>8} {:>8}",
        "phase", "query (lane)", "source_refs", "sections", "selected"
    );
    println!(
        "{:<6} {:<24} {:>11} {:>8} {:>8}",
        "before",
        "scoped, no lineage",
        before.source_refs.len(),
        before.sections.len(),
        before.selected_ids.len()
    );
    println!(
        "{:<6} {:<24} {:>11} {:>8} {:>8}",
        "after",
        "rx-1042 exact+lineage",
        after.source_refs.len(),
        after.sections.len(),
        after.selected_ids.len()
    );
    Ok(())
}
