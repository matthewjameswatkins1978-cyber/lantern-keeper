use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use lighting_core::{EpistemicRepository, ReconciliationAction, Stance};
use lighting_service::{
    EpistemicService,
    epistemic_dto::{
        BeliefRequest, ClaimRequest, CorrectionRequest, PredicateDefinitionRequest,
        ReconcileClaimRequest, RelationRequest,
    },
};
use lighting_store_surreal::{StoreConfig, SurrealEpistemicRepository, SurrealStore};
use lighting_store_surreal::{SurrealMemoryPathRepository, SurrealSourceRepository};
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

#[tokio::test]
async fn correction_records_evidence_and_reconciles_one_target()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store.clone());
    repository.migrate().await?;
    let source_repository = SurrealSourceRepository::new(store.clone());
    source_repository.migrate().await?;
    let memory_path_repository = SurrealMemoryPathRepository::new(store);
    memory_path_repository.migrate().await?;
    let service = EpistemicService::new_with_evidence(
        Arc::new(repository.clone()),
        Arc::new(source_repository),
        Arc::new(memory_path_repository),
    );

    service
        .create_predicate_definition(PredicateDefinitionRequest {
            key: "preferred_editor".to_owned(),
            aliases: Vec::new(),
            value_type: "text".to_owned(),
            allowed_dimensions: vec!["os".to_owned()],
            description: "Matthew's preferred editor".to_owned(),
            status: lighting_core::PredicateDefinitionStatus::Active,
            actor_id: "lucy".to_owned(),
        })
        .await?;
    let first = service
        .capture_claim(claim_request("VS Code", Utc::now() - Duration::minutes(2)))
        .await?;
    let created = service
        .reconcile_claim(
            first.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?;
    let old = created.belief.expect("initial belief should exist");
    let dependent = service
        .create_belief(BeliefRequest {
            holder_key: "lucy".to_owned(),
            subject_key: "editor_setup".to_owned(),
            predicate_key: "uses_editor".to_owned(),
            current_value: "Zed plugins".to_owned(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            confidence: 0.7,
            trust_class: Some(lighting_core::TrustClass::Derived),
            known_from: None,
            valid_from: None,
        })
        .await?;
    service
        .store_relation(RelationRequest {
            in_id: old.id.to_string(),
            out_id: dependent.id.to_string(),
            relation_type: "depends_on".to_owned(),
            origin: "test".to_owned(),
            confidence: 1.0,
            resolved: true,
        })
        .await?;

    let correction = service
        .record_correction(CorrectionRequest {
            target_belief_id: old.id.to_string(),
            correction_text: "No, I use Zed on Mac.".to_owned(),
            replacement_value: "Zed".to_owned(),
            scope: BTreeMap::new(),
        })
        .await?;
    assert!(!correction.source_id.is_empty());
    assert!(!correction.episode_id.is_empty());
    assert_eq!(correction.claim.originator_actor_id, "matthew");
    assert_eq!(
        correction.claim.source_id.as_deref(),
        Some(correction.source_id.as_str())
    );
    assert_eq!(
        correction.claim.episode_id.as_deref(),
        Some(correction.episode_id.as_str())
    );
    assert_eq!(
        correction.reconciliation.decision.action,
        ReconciliationAction::Supersede
    );
    assert_eq!(
        correction
            .reconciliation
            .belief
            .as_ref()
            .expect("replacement belief should exist")
            .current_value,
        "Zed"
    );
    assert_eq!(
        repository
            .get_belief(&old.id)
            .await?
            .expect("old belief remains")
            .state,
        lighting_core::BeliefState::Superseded
    );
    assert!(
        repository
            .get_belief(&dependent.id)
            .await?
            .expect("dependent belief remains")
            .stale,
        "correction must invalidate dependent projections"
    );

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
