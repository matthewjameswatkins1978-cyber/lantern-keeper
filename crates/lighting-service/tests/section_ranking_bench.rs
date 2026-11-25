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

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::Deserialize;

use lighting_core::{
    Belief, BeliefState, EpistemicRepository, NewBelief, NewSource, Source, SourceContent,
    SourceKind, SourceRepository, SourceTitle, TrustClass,
};
use lighting_service::epistemic_dto::ContextCompileRequest;
use lighting_service::{EpistemicService, source_ops::SourceService};
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealSourceRepository, SurrealStore,
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
