use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use lighting_core::{EpistemicRepository, ReconciliationAction, Stance};
use lighting_service::{
    EpistemicService,
    epistemic_dto::{ClaimRequest, PredicateDefinitionRequest, ReconcileClaimRequest},
};
use lighting_store_surreal::{StoreConfig, SurrealEpistemicRepository, SurrealStore};
use uuid::Uuid;

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "epistemic_reconciliation".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-epistemic-reconciliation-{}",
        Uuid::new_v4().simple()
    ))
}

fn claim_request(value: &str, known_at: chrono::DateTime<Utc>) -> ClaimRequest {
    ClaimRequest {
        subject_key: "matthew".to_owned(),
        value: value.to_owned(),
        source_id: Some("source:editor-preference".to_owned()),
        episode_id: Some("episode:editor-preference".to_owned()),
        evidence_span: None,
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: None,
        scope: BTreeMap::from([(String::from("os"), String::from("Mac"))]),
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        stance: Some(Stance::Endorsing),
        framing_path: Vec::new(),
        confidence: 0.95,
        known_at: Some(known_at),
        valid_from: Some(known_at),
        valid_to: None,
        extractor: "test".to_owned(),
        extractor_version: "1".to_owned(),
    }
}

#[tokio::test]
async fn claim_reconciliation_persists_history_and_lineage()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    let service = EpistemicService::new(Arc::new(repository.clone()));

    service
        .create_predicate_definition(PredicateDefinitionRequest {
            key: "preferred_editor".to_owned(),
            aliases: vec!["editor".to_owned()],
            value_type: "text".to_owned(),
            allowed_dimensions: vec!["os".to_owned()],
            description: "Matthew's preferred editor".to_owned(),
            status: lighting_core::PredicateDefinitionStatus::Active,
            actor_id: "lucy".to_owned(),
        })
        .await?;

    let first_time = Utc::now() - Duration::minutes(2);
    let first = service
        .capture_claim(claim_request("Zed", first_time))
        .await?;
    let created = service
        .reconcile_claim(
            first.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?;
    assert_eq!(created.decision.action, ReconciliationAction::Create);
    let old = created.belief.expect("create must return a belief");
    assert_eq!(old.lineage.supporting_claim_ids, vec![first.id.to_string()]);

    let second_time = first_time + Duration::minutes(1);
    let second = service
        .capture_claim(claim_request("another editor", second_time))
        .await?;
    let superseded = service
        .reconcile_claim(
            second.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?;
    assert_eq!(superseded.decision.action, ReconciliationAction::Supersede);
    let current = superseded
        .belief
        .expect("supersession must return replacement");
    assert_ne!(current.id, old.id);
    assert_eq!(current.current_value, "another editor");
    assert_eq!(current.lineage.prior_belief_id, Some(old.id.to_string()));

    let historical = repository
        .get_belief(&old.id)
        .await?
        .expect("historical projection must remain");
    assert_eq!(historical.state, lighting_core::BeliefState::Superseded);
    assert_eq!(historical.current_value, "Zed");
    assert_eq!(
        repository.get_belief(&current.id).await?,
        Some(current.clone())
    );

    let old_revisions = repository.list_belief_revisions(&old.id).await?;
    let current_revisions = repository.list_belief_revisions(&current.id).await?;
    assert_eq!(old_revisions.len(), 2, "create plus supersession history");
    assert_eq!(
        current_revisions.len(),
        1,
        "replacement has one current revision"
    );
    assert_eq!(old_revisions[0].new_value.as_deref(), Some("Zed"));
    assert_eq!(old_revisions[1].new_value, None);
    assert_eq!(current_revisions[0].previous_value.as_deref(), Some("Zed"));
    assert_eq!(
        current_revisions[0].new_value.as_deref(),
        Some("another editor")
    );
    assert!(
        old_revisions
            .iter()
            .all(|revision| revision.claim_id.is_some())
    );

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
