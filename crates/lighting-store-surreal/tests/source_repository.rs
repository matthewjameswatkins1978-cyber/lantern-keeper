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
