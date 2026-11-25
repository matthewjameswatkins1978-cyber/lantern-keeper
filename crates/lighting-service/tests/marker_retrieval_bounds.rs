//! CONTENT-1 — marker retrieval bounds.
//!
//! Pins the service-level cap on Marker→Episode links (the warning used to
//! claim a hard limit while nothing truncated at service level):
//!
//! 1. over-cap: >50 linked episodes resolve to exactly 50 processed entries,
//!    `truncated: true`, one honest cap warning;
//! 2. under-cap: unchanged shape — no warning, `truncated: false`;
//! 3. every returned excerpt/source pointer is byte-for-byte the historical
//!    text, across all three revision paths (current-revision, exact rebase,
//!    ambiguous rebase retained);
//! 4. source text is never altered by retrieval (read-only).
//!
//! Fixture note: with this adapter the store's link query already stops at
//! 50 rows, so "over-cap" is proven end-to-end (51+ links requested from the
//! store, 50 processed, flag set). The service cap exists so truncation is a
//! tested service guarantee instead of an adapter accident, and so adapters
//! without their own limit stay bounded.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use lighting_core::{
    Episode, EpisodeMarkerLink, EpisodeTitle, Marker, MemoryPathRepository, NewSource, Source,
    SourceContent, SourceKind, SourceRepository, SourceTitle, StoreMarkerResult, StoreSourceResult,
};
use lighting_service::marker_retrieval_ops::MarkerRetrievalService;
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

const MARKER_PHRASE: &str = "content bounds marker";
const TITLE_A: &str = "CONTENT-1 bounds note A";
const TITLE_B: &str = "CONTENT-1 bounds note B";

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-content1-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "m1_marker_bounds".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

/// Expected retrieval outcome for one seeded episode.
#[derive(Debug, Clone)]
struct EpisodeExpectation {
    source_id: String,
    content_source_id: String,
    latest_source_id: Option<String>,
    start_byte: usize,
    end_byte: usize,
    excerpt: String,
}

struct Fixture {
    sources: Arc<SurrealSourceRepository>,
    service: MarkerRetrievalService,
    a_rev1: String,
    a_rev2: String,
    b_content: String,
    a_rev1_id: String,
    a_rev2_id: String,
    b_id: String,
    expected: BTreeMap<String, EpisodeExpectation>,
}

async fn repos(
    label: &str,
) -> Result<
    (
        SurrealStore,
        Arc<SurrealMemoryPathRepository>,
        Arc<SurrealSourceRepository>,
    ),
    Box<dyn std::error::Error>,
> {
    let store = SurrealStore::connect(&embedded_config(test_path(label))).await?;
    store.initialise_schema().await?;
    let memory = Arc::new(SurrealMemoryPathRepository::new(store.clone()));
    memory.migrate().await?;
    let sources = Arc::new(SurrealSourceRepository::new(store.clone()));
    sources.migrate().await?;
    Ok((store, memory, sources))
}

async fn store_source(
    sources: &SurrealSourceRepository,
    title: &str,
    content: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let source = Source::create(NewSource {
        kind: SourceKind::Markdown,
        title: SourceTitle::new(title.to_owned())
            .map_err(|error| format!("bad source title: {error}"))?,
        content: SourceContent::new(content.to_owned())
            .map_err(|error| format!("bad source content: {error}"))?,
    });
    let id = source.id().as_str().to_owned();
    match sources.store(source).await? {
        StoreSourceResult::Stored(_) => Ok(id),
        StoreSourceResult::Duplicate { existing_id, .. } => Ok(existing_id.as_str().to_owned()),
    }
}

async fn create_marker(
    memory: &SurrealMemoryPathRepository,
) -> Result<String, Box<dyn std::error::Error>> {
    let marker = Marker::new(MARKER_PHRASE).map_err(|error| format!("bad marker: {error}"))?;
    let id = marker.id().as_str().to_owned();
    match memory.create_marker(marker).await? {
        StoreMarkerResult::Created(_) | StoreMarkerResult::Existing(_) => Ok(id),
    }
}

async fn link(
    memory: &SurrealMemoryPathRepository,
    episode_id: lighting_core::EpisodeId,
    marker_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let marker = lighting_core::MarkerId::new(marker_id.to_owned())
        .map_err(|error| format!("bad marker id: {error}"))?;
    memory
        .link_episode_marker(EpisodeMarkerLink::new(episode_id, marker))
        .await?;
    Ok(())
}

async fn create_episode(
    memory: &SurrealMemoryPathRepository,
    title: &str,
    source_id: &lighting_core::SourceId,
    content: &str,
    start: usize,
    end: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let range_content = SourceContent::new(content.to_owned())
        .map_err(|error| format!("bad range content: {error}"))?;
    let range = lighting_core::SourceRange::new(source_id.clone(), start, end, &range_content)
        .map_err(|error| format!("bad source range {start}..{end}: {error}"))?;
    let episode = Episode::new(
        EpisodeTitle::new(title.to_owned())
            .map_err(|error| format!("bad episode title: {error}"))?,
        range,
    );
    let id = episode.id().as_str().to_owned();
    memory.create_episode(episode).await?;
    Ok(id)
}

/// 49 segments on source A (rev1), rev2 = header prepended + the last three
/// segment texts duplicated at the end; 3 segments on single-revision source B.
/// 52 links total: with a 50-link cap at most 2 links are cut, so every
/// revision path (unique rebase 46, ambiguous rebase 3, current-only 3)
/// retains at least one surviving member in the worst-case cut.
async fn seed_over_cap(label: &str) -> Result<Fixture, Box<dyn std::error::Error>> {
    let (_store, memory, sources) = repos(label).await?;

    let mut a_rev1 = String::new();
    let mut a_ranges: Vec<(usize, usize)> = Vec::new();
    for index in 0..49usize {
        let segment = format!("seg-{index:03}-payload-line-ok\n");
        let start = a_rev1.len();
        a_rev1.push_str(&segment);
        a_ranges.push((start, a_rev1.len()));
    }
    let header = "REV2-HEADER-LINE\n";
    let mut a_rev2 = String::from(header);
    a_rev2.push_str(&a_rev1);
    for index in 46..49usize {
        a_rev2.push_str(&format!("seg-{index:03}-payload-line-ok\n"));
    }
    let mut b_content = String::new();
    let mut b_ranges: Vec<(usize, usize)> = Vec::new();
    for index in 0..3usize {
        let segment = format!("bee-{index:03}-payload-line-ok\n");
        let start = b_content.len();
        b_content.push_str(&segment);
        b_ranges.push((start, b_content.len()));
    }

    let a_rev1_id = store_source(&sources, TITLE_A, &a_rev1).await?;
    let a_rev2_id = store_source(&sources, TITLE_A, &a_rev2).await?;
    assert_ne!(a_rev1_id, a_rev2_id, "rev2 must be a distinct revision");
    let b_id = store_source(&sources, TITLE_B, &b_content).await?;
    let marker_id = create_marker(&memory).await?;

    let a_rev1_source = lighting_core::SourceId::parse(&a_rev1_id).ok_or("a_rev1 id must parse")?;
    let b_source = lighting_core::SourceId::parse(&b_id).ok_or("b id must parse")?;

    let mut expected = BTreeMap::new();
    for (index, (start, end)) in a_ranges.iter().enumerate() {
        let episode_id = create_episode(
            &memory,
            &format!("bounds-a-ep-{index:03}"),
            &a_rev1_source,
            &a_rev1,
            *start,
            *end,
        )
        .await?;
        link(
            &memory,
            episode_id.parse().map_err(|_| "episode id must parse")?,
            &marker_id,
        )
        .await?;
        let excerpt = a_rev1[*start..*end].to_owned();
        let ambiguous = index >= 46;
        expected.insert(
            episode_id.clone(),
            EpisodeExpectation {
                source_id: a_rev1_id.clone(),
                content_source_id: if ambiguous {
                    a_rev1_id.clone()
                } else {
                    a_rev2_id.clone()
                },
                latest_source_id: ambiguous.then(|| a_rev2_id.clone()),
                start_byte: if ambiguous {
                    *start
                } else {
                    *start + header.len()
                },
                end_byte: if ambiguous { *end } else { *end + header.len() },
                excerpt,
            },
        );
    }
    for (index, (start, end)) in b_ranges.iter().enumerate() {
        let episode_id = create_episode(
            &memory,
            &format!("bounds-b-ep-{index:03}"),
            &b_source,
            &b_content,
            *start,
            *end,
        )
        .await?;
        link(
            &memory,
            episode_id.parse().map_err(|_| "episode id must parse")?,
            &marker_id,
        )
        .await?;
        expected.insert(
            episode_id.clone(),
            EpisodeExpectation {
                source_id: b_id.clone(),
                content_source_id: b_id.clone(),
                latest_source_id: None,
                start_byte: *start,
                end_byte: *end,
                excerpt: b_content[*start..*end].to_owned(),
            },
        );
    }
    assert_eq!(expected.len(), 52, "fixture must exceed the 50-link cap");

    let service = MarkerRetrievalService::new(memory.clone(), sources.clone());
    Ok(Fixture {
        sources,
        service,
        a_rev1,
        a_rev2,
        b_content,
        a_rev1_id,
        a_rev2_id,
        b_id,
        expected,
    })
}

/// Three episodes on a two-revision source: well under the cap.
async fn seed_small(label: &str) -> Result<Fixture, Box<dyn std::error::Error>> {
    let (_store, memory, sources) = repos(label).await?;

    let mut a_rev1 = String::new();
    let mut a_ranges: Vec<(usize, usize)> = Vec::new();
    for index in 0..3usize {
        let segment = format!("small-{index:03}-payload-line-ok\n");
        let start = a_rev1.len();
        a_rev1.push_str(&segment);
        a_ranges.push((start, a_rev1.len()));
    }
    let header = "REV2-HEADER-LINE\n";
    let a_rev2 = format!("{header}{a_rev1}");
    // Stored (episode-less) so the fixture keeps a single-revision source B
    // reference; content is non-empty because SourceContent rejects blanks.
    let b_content = String::from("bee-filler-line\n");

    let a_rev1_id = store_source(&sources, TITLE_A, &a_rev1).await?;
    let a_rev2_id = store_source(&sources, TITLE_A, &a_rev2).await?;
    let b_id = store_source(&sources, TITLE_B, &b_content).await?;
    let marker_id = create_marker(&memory).await?;
    let a_source = lighting_core::SourceId::parse(&a_rev1_id).ok_or("id must parse")?;

    let mut expected = BTreeMap::new();
    for (index, (start, end)) in a_ranges.iter().enumerate() {
        let episode_id = create_episode(
            &memory,
            &format!("small-ep-{index:03}"),
            &a_source,
            &a_rev1,
            *start,
            *end,
        )
        .await?;
        link(
            &memory,
            episode_id.parse().map_err(|_| "episode id must parse")?,
            &marker_id,
        )
        .await?;
        expected.insert(
            episode_id.clone(),
            EpisodeExpectation {
                source_id: a_rev1_id.clone(),
                content_source_id: a_rev2_id.clone(),
                latest_source_id: None,
                start_byte: *start + header.len(),
                end_byte: *end + header.len(),
                excerpt: a_rev1[*start..*end].to_owned(),
            },
        );
    }

    let service = MarkerRetrievalService::new(memory.clone(), sources.clone());
    Ok(Fixture {
        sources,
        service,
        a_rev1,
        a_rev2,
        b_content,
        a_rev1_id,
        a_rev2_id,
        b_id,
        expected,
    })
}

fn assert_returned_matches(
    fixture: &Fixture,
    returned: &[lighting_service::marker_retrieval_dto::RetrievedEpisode],
) {
    for episode in returned {
        let want = fixture
            .expected
            .get(&episode.episode_id)
            .unwrap_or_else(|| panic!("unexpected episode {} returned", episode.episode_id));
        assert_eq!(
            episode.source_id, want.source_id,
            "historical source id for {}",
            episode.episode_id
        );
        assert_eq!(
            episode.content_source_id, want.content_source_id,
            "content source for {}",
            episode.episode_id
        );
        assert_eq!(
            episode.latest_source_id, want.latest_source_id,
            "latest source for {}",
            episode.episode_id
        );
        assert_eq!(
            episode.start_byte, want.start_byte,
            "start for {}",
            episode.episode_id
        );
        assert_eq!(
            episode.end_byte, want.end_byte,
            "end for {}",
            episode.episode_id
        );
        assert_eq!(
            episode.excerpt, want.excerpt,
            "excerpt must be byte-for-byte historical text for {}",
            episode.episode_id
        );
        assert!(episode.source_id == fixture.a_rev1_id || episode.source_id == fixture.b_id);
    }
}

#[tokio::test]
async fn over_cap_returns_exactly_50_with_truncated_flag() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = seed_over_cap("over-cap").await?;
    let response = fixture.service.retrieve(MARKER_PHRASE).await?;

    assert_eq!(
        response.episodes.len(),
        50,
        "over-cap retrieval must process exactly MAX_MARKER_EPISODES links"
    );
    assert!(response.truncated, "reaching the cap must set truncated");
    assert_eq!(response.warnings.len(), 1, "one honest cap warning");
    assert!(
        response.warnings[0].contains("50") && response.warnings[0].contains("cap"),
        "warning must name the cap honestly, got: {}",
        response.warnings[0]
    );
    assert!(response.marker.is_some());

    assert_returned_matches(&fixture, &response.episodes);

    // Every revision path survives any 2-of-52 cut.
    assert!(
        response
            .episodes
            .iter()
            .any(|episode| episode.content_source_id == fixture.a_rev2_id
                && episode.latest_source_id.is_none()
                && episode.start_byte >= "REV2-HEADER-LINE\n".len()),
        "unique-rebase path must appear in the capped result"
    );
    assert!(
        response
            .episodes
            .iter()
            .any(
                |episode| episode.latest_source_id.as_deref() == Some(fixture.a_rev2_id.as_str())
                    && episode.content_source_id == fixture.a_rev1_id
            ),
        "ambiguous-rebase path must appear in the capped result"
    );
    assert!(
        response
            .episodes
            .iter()
            .any(|episode| episode.content_source_id == fixture.b_id
                && episode.latest_source_id.is_none()),
        "current-revision path must appear in the capped result"
    );

    // Read-only guarantee: source text untouched by retrieval.
    let rev1_id = lighting_core::SourceId::parse(&fixture.a_rev1_id).ok_or("rev1 id")?;
    let rev2_id = lighting_core::SourceId::parse(&fixture.a_rev2_id).ok_or("rev2 id")?;
    let bee_id = lighting_core::SourceId::parse(&fixture.b_id).ok_or("source B id")?;
    let rev1 = fixture.sources.get(&rev1_id).await?.expect("rev1 persists");
    let rev2 = fixture.sources.get(&rev2_id).await?.expect("rev2 persists");
    let bee = fixture
        .sources
        .get(&bee_id)
        .await?
        .expect("source B persists");
    assert_eq!(
        rev1.content().as_str(),
        fixture.a_rev1,
        "rev1 bytes must be unaltered"
    );
    assert_eq!(
        rev2.content().as_str(),
        fixture.a_rev2,
        "rev2 bytes must be unaltered"
    );
    assert_eq!(
        bee.content().as_str(),
        fixture.b_content,
        "source B bytes must be unaltered"
    );
    Ok(())
}

#[tokio::test]
async fn under_cap_is_unchanged_and_unflagged() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = seed_small("under-cap").await?;
    let response = fixture.service.retrieve(MARKER_PHRASE).await?;

    assert_eq!(response.episodes.len(), 3);
    assert!(
        !response.truncated,
        "under-cap retrieval must not flag truncation"
    );
    assert!(
        response.warnings.is_empty(),
        "under-cap responses carry no warnings, got {:?}",
        response.warnings
    );
    assert_eq!(response.query, MARKER_PHRASE);

    // Deterministic comparison: order-independent against the fixture map.
    let mut returned: Vec<_> = response.episodes.iter().collect();
    returned.sort_by(|a, b| a.episode_id.cmp(&b.episode_id));
    let mut wanted: Vec<_> = fixture.expected.keys().collect();
    wanted.sort();
    let returned_ids: Vec<&String> = returned.iter().map(|episode| &episode.episode_id).collect();
    assert_eq!(
        returned_ids, wanted,
        "all seeded episodes returned exactly once"
    );

    assert_returned_matches(&fixture, &response.episodes);
    // Under-cap rebase still byte-for-byte.
    for episode in &response.episodes {
        assert_eq!(episode.content_source_id, fixture.a_rev2_id);
        assert!(episode.start_byte >= "REV2-HEADER-LINE\n".len());
    }
    Ok(())
}
