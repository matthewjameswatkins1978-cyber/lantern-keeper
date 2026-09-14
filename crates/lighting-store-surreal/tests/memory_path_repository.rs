//! Live SurrealDB integration tests for memory-path persistence.

use lighting_core::{
    Episode, EpisodeMarkerLink, EpisodeProjectLink, EpisodeTitle, Marker, MemoryPathRepository,
    MemoryPathRepositoryError, NewSource, Project, ProjectLinkKind, ProjectName, ProjectStatus,
    Source, SourceContent, SourceId, SourceKind, SourceRange, SourceRepository, SourceTitle,
    StoreMarkerResult,
};
use lighting_store_surreal::{
    StoreConfig, SurrealMemoryPathRepository, SurrealSourceRepository, SurrealStore,
};
use uuid::Uuid;

fn skip_integration_tests() -> bool {
    matches!(
        std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn test_config() -> StoreConfig {
    dotenvy::dotenv().ok();
    let mut config = StoreConfig::from_env();
    config.storage = "remote-surreal".to_owned();
    config.namespace = "lighting_test".to_owned();
    config.database = format!("lighting_mp_test_{}", Uuid::new_v4().simple());
    config
}

async fn connect() -> (SurrealSourceRepository, SurrealMemoryPathRepository) {
    let config = test_config();
    let store = SurrealStore::connect(&config)
        .await
        .unwrap_or_else(|e| panic!("failed to connect: {e}"));
    let src = SurrealSourceRepository::new(store.clone());
    src.migrate().await.expect("source migration");
    let mp = SurrealMemoryPathRepository::new(store);
    mp.migrate().await.expect("memory-path migration");
    (src, mp)
}

fn mk_source(title: &str, content: &str) -> Source {
    Source::create(NewSource {
        kind: SourceKind::Markdown,
        title: SourceTitle::new(title).unwrap(),
        content: SourceContent::new(content).unwrap(),
    })
}

#[tokio::test]
async fn create_and_get_project() {
    if skip_integration_tests() {
        return;
    }
    let (_, mp) = connect().await;
    let p = Project::new(
        ProjectName::new("Lantern Keeper").unwrap(),
        ProjectStatus::Active,
    );
    let stored = mp.create_project(p.clone()).await.unwrap();
    assert_eq!(stored.name().as_str(), "Lantern Keeper");
    let found = mp.get_project(p.id()).await.unwrap().unwrap();
    assert_eq!(found.name().as_str(), "Lantern Keeper");
    assert_eq!(found.status(), ProjectStatus::Active);
}

#[tokio::test]
async fn create_and_find_marker_by_lookup() {
    if skip_integration_tests() {
        return;
    }
    let (_, mp) = connect().await;
    let m = Marker::new("IMPORTANT").unwrap();
    let result = mp.create_marker(m).await.unwrap();
    assert!(matches!(result, StoreMarkerResult::Created(_)));
    if let StoreMarkerResult::Created(ref m) = result {
        let found = mp
            .find_marker_by_lookup("important")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id(), m.id());
        assert_eq!(found.lookup_key(), "important");
    }
}

#[tokio::test]
async fn duplicate_marker_lookup_returns_existing() {
    if skip_integration_tests() {
        return;
    }
    let (_, mp) = connect().await;
    let a = Marker::new("  Dup  ").unwrap();
    let r1 = mp.create_marker(a).await.unwrap();
    let id1 = match &r1 {
        StoreMarkerResult::Created(m) => m.id().clone(),
        StoreMarkerResult::Existing(m) => m.id().clone(),
    };
    let b = Marker::new("dup").unwrap();
    let r2 = mp.create_marker(b).await.unwrap();
    match r2 {
        StoreMarkerResult::Existing(m) => assert_eq!(m.id(), &id1),
        _ => panic!("should return existing"),
    }
}

#[tokio::test]
async fn episode_stores_and_recovers_exact_range() {
    if skip_integration_tests() {
        return;
    }
    let (src, mp) = connect().await;
    let content = "# Handbook\n\n  leading\r\n\n\ntrailing  \t";
    let source = mk_source("Handbook", content);
    let source = match src.store(source).await.unwrap() {
        lighting_core::StoreSourceResult::Stored(s) => s,
        _ => panic!(),
    };
    let range = SourceRange::new(source.id().clone(), 0, 10, source.content()).unwrap();
    let ep = Episode::new(EpisodeTitle::new("Intro").unwrap(), range);
    let stored = mp.create_episode(ep).await.unwrap();
    let retrieved = mp.get_episode(stored.id()).await.unwrap().unwrap();
    assert_eq!(retrieved.title().as_str(), "Intro");
    let slice = retrieved.source_range().slice(source.content());
    assert_eq!(slice, "# Handbook");
}

#[tokio::test]
async fn episode_creation_rejected_when_source_missing() {
    if skip_integration_tests() {
        return;
    }
    let (_, mp) = connect().await;
    let missing = SourceId::parse("00000000-0000-0000-0000-000000000000").unwrap();
    let range = SourceRange::reconstitute(missing, 0, 5);
    let ep = Episode::new(EpisodeTitle::new("Ghost").unwrap(), range);
    let err = mp.create_episode(ep).await.unwrap_err();
    assert!(matches!(err, MemoryPathRepositoryError::MissingSource));
}

#[tokio::test]
async fn link_and_list_episode_project_links() {
    if skip_integration_tests() {
        return;
    }
    let (src, mp) = connect().await;
    let source = mk_source("s", "content");
    let source = match src.store(source).await.unwrap() {
        lighting_core::StoreSourceResult::Stored(s) => s,
        _ => panic!(),
    };
    let range = SourceRange::new(source.id().clone(), 0, 7, source.content()).unwrap();
    let ep = Episode::new(EpisodeTitle::new("Ep").unwrap(), range);
    let ep = mp.create_episode(ep).await.unwrap();
    let prj = Project::new(ProjectName::new("Prj").unwrap(), ProjectStatus::Active);
    let prj = mp.create_project(prj).await.unwrap();
    mp.link_episode_project(EpisodeProjectLink::new(
        ep.id().clone(),
        prj.id().clone(),
        ProjectLinkKind::Primary,
    ))
    .await
    .unwrap();
    let links = mp.list_episode_project_links(ep.id()).await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind(), ProjectLinkKind::Primary);
}

#[tokio::test]
async fn duplicate_project_link_does_not_create_extra_records() {
    if skip_integration_tests() {
        return;
    }
    let (src, mp) = connect().await;
    let source = mk_source("s", "content");
    let source = match src.store(source).await.unwrap() {
        lighting_core::StoreSourceResult::Stored(s) => s,
        _ => panic!(),
    };
    let range = SourceRange::new(source.id().clone(), 0, 7, source.content()).unwrap();
    let ep = Episode::new(EpisodeTitle::new("Ep").unwrap(), range);
    let ep = mp.create_episode(ep).await.unwrap();
    let prj = Project::new(ProjectName::new("Prj").unwrap(), ProjectStatus::Active);
    let prj = mp.create_project(prj).await.unwrap();
    let link = EpisodeProjectLink::new(ep.id().clone(), prj.id().clone(), ProjectLinkKind::Primary);
    mp.link_episode_project(link.clone()).await.unwrap();
    mp.link_episode_project(link).await.unwrap();
    let links = mp.list_episode_project_links(ep.id()).await.unwrap();
    assert_eq!(links.len(), 1);
}

#[tokio::test]
async fn finds_project_episode_by_source_range() {
    if skip_integration_tests() {
        return;
    }
    let (src, mp) = connect().await;
    let source = mk_source("s", "alpha\nbeta\n");
    let source = match src.store(source).await.unwrap() {
        lighting_core::StoreSourceResult::Stored(s) => s,
        _ => panic!(),
    };
    let full_range = SourceRange::new(
        source.id().clone(),
        0,
        source.content().as_bytes().len(),
        source.content(),
    )
    .unwrap();
    let partial_range = SourceRange::new(source.id().clone(), 0, 5, source.content()).unwrap();
    let full_ep = mp
        .create_episode(Episode::new(EpisodeTitle::new("Full").unwrap(), full_range))
        .await
        .unwrap();
    let partial_ep = mp
        .create_episode(Episode::new(
            EpisodeTitle::new("Partial").unwrap(),
            partial_range,
        ))
        .await
        .unwrap();
    let prj = mp
        .create_project(Project::new(
            ProjectName::new("Prj").unwrap(),
            ProjectStatus::Active,
        ))
        .await
        .unwrap();
    mp.link_episode_project(EpisodeProjectLink::new(
        partial_ep.id().clone(),
        prj.id().clone(),
        ProjectLinkKind::Primary,
    ))
    .await
    .unwrap();
    mp.link_episode_project(EpisodeProjectLink::new(
        full_ep.id().clone(),
        prj.id().clone(),
        ProjectLinkKind::Primary,
    ))
    .await
    .unwrap();

    let found = mp
        .find_project_episode_by_source_range(
            prj.id(),
            source.id(),
            0,
            source.content().as_bytes().len(),
        )
        .await
        .unwrap()
        .expect("matching full-range episode");
    assert_eq!(found.id(), full_ep.id());

    let missing = mp
        .find_project_episode_by_source_range(prj.id(), source.id(), 1, 5)
        .await
        .unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn link_and_list_episode_marker_links() {
    if skip_integration_tests() {
        return;
    }
    let (src, mp) = connect().await;
    let source = mk_source("s", "content");
    let source = match src.store(source).await.unwrap() {
        lighting_core::StoreSourceResult::Stored(s) => s,
        _ => panic!(),
    };
    let range = SourceRange::new(source.id().clone(), 0, 7, source.content()).unwrap();
    let ep = Episode::new(EpisodeTitle::new("Ep").unwrap(), range);
    let ep = mp.create_episode(ep).await.unwrap();
    let mk = match mp.create_marker(Marker::new("tag").unwrap()).await.unwrap() {
        StoreMarkerResult::Created(m) => m,
        StoreMarkerResult::Existing(m) => m,
    };
    mp.link_episode_marker(EpisodeMarkerLink::new(ep.id().clone(), mk.id().clone()))
        .await
        .unwrap();
    let links = mp.list_episode_marker_links(ep.id()).await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].marker_id(), mk.id());
}

#[tokio::test]
async fn memory_path_migration_is_idempotent() {
    if skip_integration_tests() {
        return;
    }
    let (_, mp) = connect().await;
    mp.migrate().await.unwrap();
    mp.migrate().await.unwrap();
    let p = Project::new(
        ProjectName::new("After Migrations").unwrap(),
        ProjectStatus::Active,
    );
    mp.create_project(p).await.unwrap();
}
