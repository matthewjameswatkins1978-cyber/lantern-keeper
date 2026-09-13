use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use lighting_core::{
    EpistemicRepository, MemoryItemKind, Proposal, ProposalId, ReconciliationAction, Stance,
};
use lighting_service::{
    EpistemicService,
    epistemic_dto::{
        BeliefRequest, ClaimRequest, ContextCompileRequest, CorrectionRequest,
        ForemanReviewRequest, MemoryItemRequest, PredicateDefinitionRequest, ReconcileClaimRequest,
        RelationRequest,
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

    let historical_pack = service
        .compile_context(ContextCompileRequest {
            query: "Zed editor".to_owned(),
            actor: Some("lucy".to_owned()),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            intent: None,
            item_budget: 10,
        })
        .await?;
    assert_eq!(
        historical_pack.current_beliefs,
        vec![current.clone()],
        "only the active projection belongs in current context"
    );
    assert_eq!(historical_pack.historical_beliefs, vec![historical]);
    assert!(
        historical_pack
            .generated_context
            .contains("Historical beliefs")
    );
    assert!(
        historical_pack
            .source_refs
            .contains(&"source:editor-preference".to_owned())
    );
    assert!(
        historical_pack
            .episode_refs
            .contains(&"episode:editor-preference".to_owned())
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
            context_pack_id: None,
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

#[tokio::test]
async fn context_compiler_returns_bounded_typed_pack_and_persists_it()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store.clone());
    repository.migrate().await?;
    let service = EpistemicService::new(Arc::new(repository.clone()));

    let belief = service
        .create_belief(BeliefRequest {
            holder_key: "matthew".to_owned(),
            subject_key: "matthew".to_owned(),
            predicate_key: "preferred_editor".to_owned(),
            current_value: "Zed".to_owned(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            confidence: 0.9,
            trust_class: Some(lighting_core::TrustClass::Direct),
            known_from: None,
            valid_from: None,
        })
        .await?;
    let memory = service
        .remember_soft(MemoryItemRequest {
            kind: MemoryItemKind::CreativeSeed,
            content: "An odd idea about ants playing strategy games".to_owned(),
            source_id: None,
            episode_id: None,
            originator_actor_id: Some("matthew".to_owned()),
            transmitter_actor_id: None,
            holder_actor_id: Some("shared:matthew-lucy".to_owned()),
            salience: 0.8,
        })
        .await?;

    let pack = service
        .compile_context(ContextCompileRequest {
            query: "preferred editor ants games".to_owned(),
            actor: Some("lucy".to_owned()),
            scope: BTreeMap::from([(String::from("os"), String::from("Mac"))]),
            intent: Some("retrieve useful context".to_owned()),
            item_budget: 2,
        })
        .await?;

    assert_eq!(pack.retrieval_trace.item_budget, 2);
    assert!(pack.selected_ids.len() <= 2);
    assert_eq!(pack.current_beliefs, vec![belief]);
    assert_eq!(pack.soft_memories, vec![memory]);
    assert!(pack.retrieval_trace.candidate_scores.len() >= 2);
    assert!(pack.episode_refs.is_empty());
    assert!(pack.generated_context.contains("Matthew beliefs"));
    assert!(
        pack.generated_context
            .contains("ants playing strategy games")
    );
    assert_eq!(
        repository
            .get_context_pack(&pack.id)
            .await?
            .expect("compiled context pack must be durable"),
        pack
    );

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}

#[tokio::test]
async fn foreman_reviews_bounded_proposals_and_records_the_decision()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    let service = EpistemicService::new(Arc::new(repository.clone()));
    let proposal = repository
        .enqueue_proposal(Proposal {
            id: ProposalId::new(Uuid::new_v4().to_string())?,
            proposal_type: "candidate_pattern".to_owned(),
            proposed_by: "dreamer".to_owned(),
            target_ids: vec!["memory-1".to_owned()],
            payload: "old".to_owned(),
            status: "pending".to_owned(),
            created_at: Utc::now(),
            decided_at: None,
        })
        .await?;

    let queue = service.foreman_queue(10).await?;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].id, proposal.id);
    let reviewed = service
        .foreman_review(
            proposal.id.as_str(),
            ForemanReviewRequest {
                decision: "modify".to_owned(),
                payload: Some("updated".to_owned()),
            },
        )
        .await?;
    assert_eq!(reviewed.status, "pending");
    assert_eq!(reviewed.payload, "updated");
    assert_eq!(service.foreman_queue(10).await?.len(), 1);

    let accepted = service
        .foreman_review(
            proposal.id.as_str(),
            ForemanReviewRequest {
                decision: "accept".to_owned(),
                payload: None,
            },
        )
        .await?;
    assert_eq!(accepted.status, "accepted");
    assert!(accepted.decided_at.is_some());
    assert!(service.foreman_queue(10).await?.is_empty());

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
