use lighting_core::{
    NewSource, Source, SourceContent, SourceId, SourceKind, SourceRepository, SourceTitle,
    StoreSourceResult,
};
use lighting_store_surreal::{source_store::SurrealSourceRepository, StoreConfig, SurrealStore};
use uuid::Uuid;

fn skip_integration_tests() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn markdown_source(title: &str, content: &str) -> Source {
    Source::create(NewSource {
        kind: SourceKind::Markdown,
        title: SourceTitle::new(title).expect("title should be valid"),
        content: SourceContent::new(content).expect("content should be valid"),
    })
}

fn test_config() -> StoreConfig {
    dotenvy::dotenv().ok();
    let mut config = StoreConfig::from_env();
    config.namespace = "lighting_test".to_owned();
    config.database = format!("lighting_source_test_{}", Uuid::new_v4().simple());
    config
}

async fn connect_repository() -> SurrealSourceRepository {
    let config = test_config();
    let store = SurrealStore::connect(&config)
        .await
        .unwrap_or_else(|error| panic!("failed to connect to local SurrealDB for source repository tests. Start it with the documented command, or set LIGHTING_SKIP_INTEGRATION_TESTS=1 to skip. Error: {error}"));
    let repo = SurrealSourceRepository::new(store);
    repo.migrate()
        .await
        .expect("schema migration should succeed");
    repo
}

#[tokio::test]
async fn stores_and_retrieves_markdown_source_exactly() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let content = "# Heading\n\n  leading\r\n\n\ntrailing  \t";
    let source = markdown_source("Exact Markdown", content);

    let stored = repo
        .store(source.clone())
        .await
        .expect("store should succeed");
    assert!(matches!(stored, StoreSourceResult::Stored(_)));

    let retrieved = repo
        .get(source.id())
        .await
        .expect("get should succeed")
        .expect("source should exist");

    assert_eq!(retrieved.kind(), SourceKind::Markdown);
    assert_eq!(retrieved.title().as_str(), "Exact Markdown");
    assert_eq!(retrieved.content().as_str(), content);
    assert_eq!(retrieved.content().as_bytes(), content.as_bytes());
    assert_eq!(retrieved.fingerprint(), source.fingerprint());
}

#[tokio::test]
async fn duplicate_content_returns_existing_id_without_new_record() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let content = "duplicate body";
    let first = markdown_source("First", content);
    let second = markdown_source("Second", content);

    let first_result = repo
        .store(first.clone())
        .await
        .expect("first store should succeed");
    let first_id = match first_result {
        StoreSourceResult::Stored(source) => source.id().clone(),
        StoreSourceResult::Duplicate { .. } => panic!("first store should not be a duplicate"),
    };

    let second_result = repo
        .store(second.clone())
        .await
        .expect("second store should succeed");
    match second_result {
        StoreSourceResult::Duplicate {
            existing_id,
            attempted,
        } => {
            assert_eq!(existing_id, first_id);
            assert_eq!(attempted.content().as_str(), content);
        }
        StoreSourceResult::Stored(_) => panic!("second store should be a duplicate"),
    }

    // Ensure only one record exists for this fingerprint.
    let count: Vec<surrealdb::types::Object> = repo
        .raw_query("SELECT count() FROM source GROUP ALL")
        .await
        .expect("count query should succeed")
        .take(0)
        .expect("count result should decode");
    let total: i64 = count
        .first()
        .and_then(|obj| obj.get("count"))
        .map(|value| value.clone().into_int())
        .expect("count should be present")
        .expect("count should be an integer");
    assert_eq!(total, 1);
}

#[tokio::test]
async fn distinct_content_creates_distinct_sources() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let first = markdown_source("A", "content one");
    let second = markdown_source("B", "content two");

    let first_result = repo
        .store(first.clone())
        .await
        .expect("first store should succeed");
    let second_result = repo
        .store(second.clone())
        .await
        .expect("second store should succeed");

    assert!(matches!(first_result, StoreSourceResult::Stored(_)));
    assert!(matches!(second_result, StoreSourceResult::Stored(_)));
    assert_ne!(first.id(), second.id());
}

#[tokio::test]
async fn missing_source_returns_none() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let missing_id =
        SourceId::parse("00000000-0000-0000-0000-000000000000").expect("valid uuid string");

    let result = repo.get(&missing_id).await.expect("get should succeed");

    assert!(result.is_none());
}

#[tokio::test]
async fn migration_is_idempotent() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    repo.migrate()
        .await
        .expect("first migration should succeed");
    repo.migrate()
        .await
        .expect("second migration should succeed");

    // A source can still be stored after repeated migrations.
    let source = markdown_source("After migrations", "content");
    let result = repo
        .store(source)
        .await
        .expect("store after repeated migrations should succeed");
    assert!(matches!(result, StoreSourceResult::Stored(_)));
}

// ── Revision detection (LK-021A) ──

/// Identity rule: (kind, title) defines "same logical source".
/// Identical content re-capture returns the existing source, not a duplicate row.
#[tokio::test]
async fn identical_recapture_returns_existing_source() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let content = "version one content";

    let first = markdown_source("Changelog", content);
    let first_result = repo
        .store(first.clone())
        .await
        .expect("first store should succeed");
    let first_id = match first_result {
        StoreSourceResult::Stored(source) => source.id().clone(),
        StoreSourceResult::Duplicate { .. } => panic!("first store should be fresh"),
    };

    // Re-capture with identical kind, title, and content.
    let second = markdown_source("Changelog", content);
    let second_result = repo
        .store(second.clone())
        .await
        .expect("second store should succeed");
    match second_result {
        StoreSourceResult::Duplicate {
            existing_id,
            attempted,
        } => {
            assert_eq!(existing_id, first_id, "should return the original id");
            assert_eq!(attempted.content().as_str(), content);
        }
        StoreSourceResult::Stored(_) => panic!("identical recapture should be a duplicate"),
    }

    // No new record was created.
    let count: Vec<surrealdb::types::Object> = repo
        .raw_query("SELECT count() FROM source WHERE title = 'Changelog' GROUP ALL")
        .await
        .expect("count query should succeed")
        .take(0)
        .expect("count result should decode");
    let total: i64 = count
        .first()
        .and_then(|obj| obj.get("count"))
        .map(|value| value.clone().into_int())
        .expect("count should be present")
        .expect("count should be an integer");
    assert_eq!(total, 1, "only one record for this logical source");
}

/// Changed content for the same logical source creates a new revision
/// with `previous_version_id` pointing to the earlier record.
#[tokio::test]
async fn changed_recapture_creates_linked_revision() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    // Initial capture.
    let v1 = markdown_source("Revision Test", "version 1");
    let v1_result = repo
        .store(v1.clone())
        .await
        .expect("v1 store should succeed");
    let v1_id = match v1_result {
        StoreSourceResult::Stored(source) => source.id().clone(),
        StoreSourceResult::Duplicate { .. } => panic!("v1 should be fresh"),
    };

    // Changed content, same kind + title.
    let v2 = markdown_source("Revision Test", "version 2 – updated");
    let v2_result = repo
        .store(v2.clone())
        .await
        .expect("v2 store should succeed");
    let v2_id = match v2_result {
        StoreSourceResult::Stored(source) => source.id().clone(),
        StoreSourceResult::Duplicate { .. } => {
            panic!("v2 with changed content should be a new revision, not a duplicate")
        }
    };

    // The new revision has a different id.
    assert_ne!(v2_id, v1_id);

    // v1 still exists with original content.
    let retrieved_v1 = repo
        .get(&v1_id)
        .await
        .expect("get should succeed")
        .expect("v1 should still exist");
    assert_eq!(
        retrieved_v1.content().as_str(),
        "version 1",
        "v1 content must be preserved"
    );
    assert!(
        retrieved_v1.previous_version_id().is_none(),
        "v1 should have no previous version"
    );

    // v2 links back to v1.
    let retrieved_v2 = repo
        .get(&v2_id)
        .await
        .expect("get should succeed")
        .expect("v2 should exist");
    assert_eq!(
        retrieved_v2.content().as_str(),
        "version 2 – updated",
        "v2 content must be the new content"
    );
    assert_eq!(
        retrieved_v2.previous_version_id(),
        Some(&v1_id),
        "v2 previous_version_id must point to v1"
    );

    // Both records coexist.
    let count: Vec<surrealdb::types::Object> = repo
        .raw_query("SELECT count() FROM source WHERE title = 'Revision Test' GROUP ALL")
        .await
        .expect("count query should succeed")
        .take(0)
        .expect("count result should decode");
    let total: i64 = count
        .first()
        .and_then(|obj| obj.get("count"))
        .map(|value| value.clone().into_int())
        .expect("count should be present")
        .expect("count should be an integer");
    assert_eq!(total, 2, "two revisions should coexist");
}

/// Initial capture of a brand-new logical source succeeds cleanly.
#[tokio::test]
async fn initial_capture_creates_fresh_source() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let source = markdown_source("Fresh Start", "initial content");

    let result = repo
        .store(source.clone())
        .await
        .expect("store should succeed");

    match result {
        StoreSourceResult::Stored(stored) => {
            assert_eq!(stored.content().as_str(), "initial content");
            assert!(stored.previous_version_id().is_none());
        }
        StoreSourceResult::Duplicate { .. } => {
            panic!("initial capture of a fresh source should not be a duplicate")
        }
    }
}

// ── Current revision resolution (LK-021B) ──

/// No matching logical source returns None.
#[tokio::test]
async fn get_current_returns_none_for_unknown_source() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let result = repo
        .get_current(
            SourceKind::Markdown,
            &SourceTitle::new("Does Not Exist").unwrap(),
        )
        .await
        .expect("get_current should succeed");

    assert!(
        result.is_none(),
        "unknown logical source should return None"
    );
}

/// A single captured source (no revisions) resolves as current.
#[tokio::test]
async fn single_source_resolves_as_current() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;
    let source = markdown_source("Solo", "solo content");
    let stored = repo
        .store(source.clone())
        .await
        .expect("store should succeed");
    let stored_id = match stored {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        StoreSourceResult::Duplicate { .. } => panic!("should be fresh"),
    };

    let current = repo
        .get_current(SourceKind::Markdown, &SourceTitle::new("Solo").unwrap())
        .await
        .expect("get_current should succeed")
        .expect("single source should be current");

    assert_eq!(current.id(), &stored_id);
    assert_eq!(current.content().as_str(), "solo content");
    assert!(current.previous_version_id().is_none());
}

/// A three-version chain resolves the final revision as current.
#[tokio::test]
async fn three_version_chain_resolves_final_as_current() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let v1 = markdown_source("Chain", "v1");
    let v1_id = match repo.store(v1).await.unwrap() {
        StoreSourceResult::Stored(s) => s.id().clone(),
        _ => panic!("v1 should be fresh"),
    };

    let v2 = markdown_source("Chain", "v2");
    let v2_id = match repo.store(v2).await.unwrap() {
        StoreSourceResult::Stored(s) => s.id().clone(),
        _ => panic!("v2 should be a new revision"),
    };

    let v3 = markdown_source("Chain", "v3 final");
    let v3_id = match repo.store(v3).await.unwrap() {
        StoreSourceResult::Stored(s) => s.id().clone(),
        _ => panic!("v3 should be a new revision"),
    };

    // v3 is current.
    let current = repo
        .get_current(SourceKind::Markdown, &SourceTitle::new("Chain").unwrap())
        .await
        .expect("get_current should succeed")
        .expect("chain should have a current revision");

    assert_eq!(current.id(), &v3_id, "v3 should be the current revision");
    assert_eq!(current.content().as_str(), "v3 final");
    // v3's previous_version_id points to v2.
    assert_eq!(
        current.previous_version_id(),
        Some(&v2_id),
        "v3 should link back to v2"
    );

    // v1 not current (referenced by v2).
    assert_ne!(&v1_id, current.id());
    // v2 not current (referenced by v3).
    assert_ne!(&v2_id, current.id());
}

/// Older revisions remain individually retrievable by ID.
#[tokio::test]
async fn older_revisions_remain_retrievable_by_id() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let v1 = markdown_source("History", "first edition");
    let v1_id = match repo.store(v1).await.unwrap() {
        StoreSourceResult::Stored(s) => s.id().clone(),
        _ => panic!("v1 should be fresh"),
    };

    let v2 = markdown_source("History", "second edition");
    let v2_id = match repo.store(v2).await.unwrap() {
        StoreSourceResult::Stored(s) => s.id().clone(),
        _ => panic!("v2 should be a new revision"),
    };

    // v1 still retrievable by ID.
    let retrieved_v1 = repo
        .get(&v1_id)
        .await
        .expect("get should succeed")
        .expect("v1 should exist");
    assert_eq!(retrieved_v1.content().as_str(), "first edition");
    assert!(retrieved_v1.previous_version_id().is_none());

    // v2 still retrievable by ID.
    let retrieved_v2 = repo
        .get(&v2_id)
        .await
        .expect("get should succeed")
        .expect("v2 should exist");
    assert_eq!(retrieved_v2.content().as_str(), "second edition");
    assert_eq!(retrieved_v2.previous_version_id(), Some(&v1_id));
}

// ── Revision-safe handoff excerpts (LK-023) ──

/// Helper: simulate the rebasing decision the service makes.
fn rebase_handoff(
    historical_source: &Source,
    current_revision: Option<&Source>,
    excerpt_pos: usize,
    excerpt_len: usize,
) -> (String, Option<String>, usize, usize) {
    let historical_excerpt =
        &historical_source.content().as_str()[excerpt_pos..excerpt_pos + excerpt_len];

    let current = match current_revision {
        Some(c) => c,
        None => {
            return (
                historical_source.id().as_str().to_owned(),
                None,
                excerpt_pos,
                excerpt_pos + excerpt_len,
            );
        }
    };

    if current.id() == historical_source.id() {
        return (
            historical_source.id().as_str().to_owned(),
            None,
            excerpt_pos,
            excerpt_pos + excerpt_len,
        );
    }

    let current_content = current.content().as_str();
    let matches: Vec<usize> = current_content
        .as_bytes()
        .windows(excerpt_len)
        .enumerate()
        .filter(|(_, w)| *w == historical_excerpt.as_bytes())
        .map(|(p, _)| p)
        .collect();

    if matches.len() == 1 {
        let pos = matches[0];
        (
            current.id().as_str().to_owned(),
            None,
            pos,
            pos + excerpt_len,
        )
    } else {
        (
            historical_source.id().as_str().to_owned(),
            Some(current.id().as_str().to_owned()),
            excerpt_pos,
            excerpt_pos + excerpt_len,
        )
    }
}

/// Unchanged Source: content_source_id == source_id, bytes unchanged.
#[tokio::test]
async fn unchanged_source_keeps_historical_bytes() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let source = markdown_source("Stable", "unchanged body text here");
    let stored = match repo.store(source.clone()).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be fresh"),
    };
    let loaded = repo.get(&stored).await.unwrap().unwrap();

    let (content_source_id, latest_source_id, start, end) = rebase_handoff(&loaded, None, 0, 10); // excerpt = "unchanged "

    assert_eq!(content_source_id, stored.as_str());
    assert_eq!(latest_source_id, None);
    assert_eq!(start, 0);
    assert_eq!(end, 10);
}

/// Revised Source with one unique match → rebased bytes, current revision used.
#[tokio::test]
async fn unique_excerpt_rebased_to_current_revision() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    // v1: "prefix OLD_DATA suffix"
    let v1 = markdown_source("Rebase Test", "prefix OLD_DATA suffix");
    let v1_id = match repo.store(v1).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be fresh"),
    };

    // v2: "header prefix OLD_DATA suffix footer" — excerpt appears once.
    let v2 = markdown_source("Rebase Test", "header prefix OLD_DATA suffix footer");
    let v2_id = match repo.store(v2).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be a new revision"),
    };

    let loaded = repo.get(&v1_id).await.unwrap().unwrap();
    let current = repo
        .get_current(loaded.kind(), loaded.title())
        .await
        .unwrap()
        .unwrap();

    // excerpt from v1: start=7, len=8 → "OLD_DATA"
    let (content_source_id, latest_source_id, start, end) =
        rebase_handoff(&loaded, Some(&current), 7, 8);

    assert_eq!(content_source_id, v2_id.as_str(), "should rebase to v2");
    assert_eq!(latest_source_id, None, "no fallback needed");
    // "prefix OLD_DATA suffix" → "header prefix OLD_DATA suffix footer"
    // "OLD_DATA" appears at byte 14 in v2.
    assert_eq!(start, 14, "rebase: OLD_DATA starts at byte 14 in v2");
    assert_eq!(end, 22, "rebase: OLD_DATA ends at byte 22 in v2");
}

/// Revised Source where excerpt is absent → retain historical, flag latest.
#[tokio::test]
async fn absent_excerpt_retains_historical_and_flags_latest() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let v1 = markdown_source("Absent Test", "deleted paragraph text");
    let v1_id = match repo.store(v1).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be fresh"),
    };

    // v2 no longer contains "deleted paragraph text" at all.
    let v2 = markdown_source("Absent Test", "completely rewritten content");
    let v2_id = match repo.store(v2).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be a new revision"),
    };

    let loaded = repo.get(&v1_id).await.unwrap().unwrap();
    let current = repo
        .get_current(loaded.kind(), loaded.title())
        .await
        .unwrap()
        .unwrap();

    // excerpt from v1: start=0, len=8 → "deleted "
    let (content_source_id, latest_source_id, start, end) =
        rebase_handoff(&loaded, Some(&current), 0, 8);

    // Falls back to historical.
    assert_eq!(
        content_source_id,
        v1_id.as_str(),
        "content_source_id stays v1"
    );
    assert_eq!(
        latest_source_id,
        Some(v2_id.as_str().to_owned()),
        "v2 flagged as latest"
    );
    assert_eq!(start, 0, "historical bytes retained");
    assert_eq!(end, 8, "historical bytes retained");
}

/// Revised Source where excerpt appears multiple times → retain historical, flag latest.
#[tokio::test]
async fn ambiguous_excerpt_retains_historical_and_flags_latest() {
    if skip_integration_tests() {
        return;
    }

    let repo = connect_repository().await;

    let v1 = markdown_source("Ambiguous Test", "dup appears here");
    let v1_id = match repo.store(v1).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be fresh"),
    };

    // v2 contains "dup" twice.
    let v2 = markdown_source("Ambiguous Test", "dup start dup end");
    let v2_id = match repo.store(v2).await.unwrap() {
        StoreSourceResult::Stored(ref s) => s.id().clone(),
        _ => panic!("should be a new revision"),
    };

    let loaded = repo.get(&v1_id).await.unwrap().unwrap();
    let current = repo
        .get_current(loaded.kind(), loaded.title())
        .await
        .unwrap()
        .unwrap();

    // excerpt from v1: start=0, len=3 → "dup"
    let (content_source_id, latest_source_id, start, end) =
        rebase_handoff(&loaded, Some(&current), 0, 3);

    // Falls back to historical.
    assert_eq!(
        content_source_id,
        v1_id.as_str(),
        "content_source_id stays v1"
    );
    assert_eq!(
        latest_source_id,
        Some(v2_id.as_str().to_owned()),
        "v2 flagged as latest"
    );
    assert_eq!(start, 0, "historical bytes retained");
    assert_eq!(end, 3, "historical bytes retained");
}
