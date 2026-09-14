//! LanternBench: deterministic behavioural checks for the current memory model.
//!
//! These cases use only synthetic records. The suite is deliberately close to
//! the service and core APIs so a green result proves behaviour rather than a
//! text-only fixture match.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use chrono::{Duration, Utc};
use lighting_core::{
    Belief, BeliefState, Claim, EpistemicRepository, Frame, FrameAction, MemoryItemKind, NewBelief,
    NewClaim, PredicateStatus, ReconciliationAction, Stance, TrustClass, reconcile_claim,
};
use lighting_service::{
    EpistemicService,
    epistemic_dto::{
        BeliefRequest, ClaimRequest, ContextCompileRequest, CorrectionRequest, MemoryItemRequest,
        PredicateDefinitionRequest, ReconcileClaimRequest, RelationRequest,
    },
};
use lighting_store_surreal::{
    StoreConfig, SurrealEpistemicRepository, SurrealMemoryPathRepository, SurrealSourceRepository,
    SurrealStore,
};
use uuid::Uuid;

fn embedded_config(path: PathBuf) -> StoreConfig {
    let mut config = StoreConfig::from_env();
    config.storage = "embedded-surrealkv".to_owned();
    config.path = path;
    config.namespace = "lanternbench".to_owned();
    config.database = "integration".to_owned();
    config.username.clear();
    config.password.clear();
    config
}

fn test_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lantern-keeper-lanternbench-{}",
        Uuid::new_v4().simple()
    ))
}

fn direct_claim(value: &str, known_at: chrono::DateTime<Utc>) -> Claim {
    Claim::new(NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: "matthew".to_owned(),
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: value.to_owned(),
        scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
        polarity: true,
        originator_actor_id: "matthew".to_owned(),
        speaker_actor_id: "matthew".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("matthew".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 1.0,
        known_at,
        valid_from: Some(known_at),
        valid_to: None,
        extractor: "lanternbench".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: known_at,
    })
    .expect("synthetic claim is valid")
}

fn belief(value: &str) -> Belief {
    let now = Utc::now();
    Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "matthew".to_owned(),
        predicate_key: "preferred_editor".to_owned(),
        current_value: value.to_owned(),
        scope: BTreeMap::new(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: now,
        valid_from: None,
        created_at: now,
    })
    .expect("synthetic belief is valid")
}

#[test]
fn lanternbench_epistemic_firewall() {
    let existing = belief("Zed");

    let mut inferred = direct_claim("Helix", Utc::now());
    inferred.originator_actor_id = "lucy".to_owned();
    inferred.speaker_actor_id = "lucy".to_owned();
    assert_eq!(
        reconcile_claim(&inferred, Some(&existing), true).action,
        ReconciliationAction::Contradict,
        "Lucy inference must not overwrite a Matthew belief"
    );

    let mut silence = direct_claim("Helix", Utc::now());
    silence.stance = Stance::Unobserved;
    assert_eq!(
        reconcile_claim(&silence, None, true).action,
        ReconciliationAction::HypothesisOnly,
        "silence must not create an active belief"
    );

    let mut quotation = direct_claim("Helix", Utc::now());
    quotation.originator_actor_id = "lucy".to_owned();
    quotation.framing_path = vec![Frame {
        actor_id: "matthew".to_owned(),
        action: FrameAction::Quoting,
        target_actor_id: Some("lucy".to_owned()),
    }];
    assert_eq!(
        reconcile_claim(&quotation, None, true).action,
        ReconciliationAction::HypothesisOnly,
        "quoted assistant text must not become a Matthew belief"
    );

    let echo = direct_claim("Zed", Utc::now());
    assert_eq!(
        reconcile_claim(&echo, Some(&existing), false).action,
        ReconciliationAction::NoChange,
        "repeated derived material must not reinforce itself"
    );
}

#[tokio::test]
async fn lanternbench_context_partitions_perspectives_and_legacy_state()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    let service = EpistemicService::new(Arc::new(repository));

    let create = |holder: &str, value: &str, trust_class| BeliefRequest {
        holder_key: holder.to_owned(),
        subject_key: "editor".to_owned(),
        predicate_key: "editor_preference".to_owned(),
        current_value: value.to_owned(),
        scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
        confidence: 0.8,
        trust_class: Some(trust_class),
        known_from: None,
        valid_from: None,
    };
    let matthew = service
        .create_belief(create("matthew", "Zed", TrustClass::Direct))
        .await?;
    let lucy = service
        .create_belief(create("lucy", "Helix", TrustClass::Hypothesis))
        .await?;
    let shared = service
        .create_belief(create(
            "shared:matthew-lucy",
            "bounded editing",
            TrustClass::Direct,
        ))
        .await?;
    let legacy = service
        .create_belief(create(
            "legacy:shared",
            "imported editor note",
            TrustClass::MigratedCanonical,
        ))
        .await?;

    let pack = service
        .compile_context(ContextCompileRequest {
            query: "editor Zed Helix bounded imported".to_owned(),
            actor: Some("lucy".to_owned()),
            project_hints: Vec::new(),
            scope: BTreeMap::from([(String::from("os"), String::from("mac"))]),
            intent: Some("perspective check".to_owned()),
            item_budget: 10,
            token_budget: 2048,
        })
        .await?;

    assert_eq!(pack.current_beliefs, vec![matthew]);
    assert_eq!(pack.lucy_beliefs, vec![lucy]);
    assert_eq!(pack.shared_beliefs, vec![shared]);
    assert_eq!(pack.legacy_beliefs, vec![legacy]);
    assert!(pack.generated_context.contains("Lucy beliefs"));
    assert!(pack.generated_context.contains("Shared beliefs"));
    assert!(pack.generated_context.contains("Legacy beliefs"));

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}

#[tokio::test]
async fn lanternbench_history_soft_recall_stale_suppression_and_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let path = test_path();
    let store = SurrealStore::connect(&embedded_config(path.clone())).await?;
    store.initialise_schema().await?;
    let repository = SurrealEpistemicRepository::new(store);
    repository.migrate().await?;
    let service = EpistemicService::new(Arc::new(repository));
    service
        .create_predicate_definition(PredicateDefinitionRequest {
            key: "preferred_editor".to_owned(),
            aliases: vec!["editor".to_owned()],
            value_type: "text".to_owned(),
            allowed_dimensions: vec!["os".to_owned()],
            description: "preferred editor".to_owned(),
            status: lighting_core::PredicateDefinitionStatus::Active,
            actor_id: "lucy".to_owned(),
        })
        .await?;

    let first_time = Utc::now() - Duration::minutes(3);
    let first = service
        .capture_claim(ClaimRequest {
            subject_key: "matthew".to_owned(),
            value: "VS Code".to_owned(),
            source_id: None,
            episode_id: None,
            evidence_span: None,
            predicate_key: Some("editor".to_owned()),
            predicate_candidate: None,
            predicate_status: None,
            scope: BTreeMap::from([(String::from("os"), String::from("mac"))]),
            polarity: true,
            originator_actor_id: "matthew".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: None,
            holder_actor_id: Some("matthew".to_owned()),
            stance: Some(Stance::Endorsing),
            framing_path: Vec::new(),
            confidence: 0.95,
            known_at: Some(first_time),
            valid_from: Some(first_time),
            valid_to: None,
            extractor: "lanternbench".to_owned(),
            extractor_version: "1".to_owned(),
        })
        .await?;
    let old = service
        .reconcile_claim(
            first.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?
        .belief
        .expect("initial belief exists");
    let second = service
        .capture_claim(ClaimRequest {
            subject_key: "matthew".to_owned(),
            value: "Zed".to_owned(),
            source_id: None,
            episode_id: None,
            evidence_span: None,
            predicate_key: Some("preferred_editor".to_owned()),
            predicate_candidate: None,
            predicate_status: None,
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            polarity: true,
            originator_actor_id: "matthew".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: None,
            holder_actor_id: Some("matthew".to_owned()),
            stance: Some(Stance::Endorsing),
            framing_path: Vec::new(),
            confidence: 0.95,
            known_at: Some(first_time + Duration::minutes(1)),
            valid_from: Some(first_time + Duration::minutes(1)),
            valid_to: None,
            extractor: "lanternbench".to_owned(),
            extractor_version: "1".to_owned(),
        })
        .await?;
    let current = service
        .reconcile_claim(
            second.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?
        .belief
        .expect("replacement belief exists");
    let stale = service
        .create_belief(BeliefRequest {
            holder_key: "lucy".to_owned(),
            subject_key: "editor_setup".to_owned(),
            predicate_key: "derived_setup".to_owned(),
            current_value: "stale editor plugin note".to_owned(),
            scope: BTreeMap::new(),
            confidence: 0.4,
            trust_class: Some(TrustClass::Derived),
            known_from: None,
            valid_from: None,
        })
        .await?;
    service
        .mark_belief_stale(stale.id.as_str(), "bench invalidation".to_owned())
        .await?;
    let memory = service
        .remember_soft(MemoryItemRequest {
            kind: MemoryItemKind::Idea,
            content: "the blind drummer game idea".to_owned(),
            source_id: None,
            episode_id: None,
            originator_actor_id: Some("matthew".to_owned()),
            transmitter_actor_id: None,
            holder_actor_id: Some("shared:matthew-lucy".to_owned()),
            salience: 0.8,
        })
        .await?;

    let current_pack = service
        .compile_context(ContextCompileRequest {
            query: "What editor does Matthew use on Mac?".to_owned(),
            actor: Some("lucy".to_owned()),
            project_hints: Vec::new(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            intent: None,
            item_budget: 10,
            token_budget: 2048,
        })
        .await?;
    assert_eq!(current_pack.current_beliefs, vec![current.clone()]);
    assert!(current_pack.historical_beliefs.is_empty());
    assert!(
        current_pack
            .omitted_stale_ids
            .contains(&stale.id.to_string())
    );
    assert!(!current_pack.generated_context.contains("VS Code"));

    let history_pack = service
        .compile_context(ContextCompileRequest {
            query: "Didn't Matthew used to use VS Code?".to_owned(),
            actor: Some("lucy".to_owned()),
            project_hints: Vec::new(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            intent: None,
            item_budget: 10,
            token_budget: 2048,
        })
        .await?;
    assert_eq!(history_pack.current_beliefs, vec![current]);
    assert_eq!(history_pack.historical_beliefs.len(), 1);
    assert_eq!(
        history_pack.historical_beliefs[0].current_value,
        old.current_value
    );
    assert_eq!(
        history_pack.historical_beliefs[0].state,
        BeliefState::Superseded
    );

    let soft_pack = service
        .compile_context(ContextCompileRequest {
            query: "What was that blind drummer idea?".to_owned(),
            actor: Some("lucy".to_owned()),
            project_hints: Vec::new(),
            scope: BTreeMap::new(),
            intent: None,
            item_budget: 1,
            token_budget: 128,
        })
        .await?;
    assert_eq!(soft_pack.soft_memories, vec![memory]);
    assert!(soft_pack.selected_ids.len() <= 1);
    assert!(soft_pack.retrieval_trace.estimated_token_usage <= 128);

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}

#[tokio::test]
async fn lanternbench_correction_trace_and_zombie_severance()
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
            aliases: vec!["editor".to_owned()],
            value_type: "text".to_owned(),
            allowed_dimensions: vec!["os".to_owned()],
            description: "preferred editor".to_owned(),
            status: lighting_core::PredicateDefinitionStatus::Active,
            actor_id: "lucy".to_owned(),
        })
        .await?;
    let initial = service
        .capture_claim(ClaimRequest {
            subject_key: "matthew".to_owned(),
            value: "VS Code".to_owned(),
            source_id: None,
            episode_id: None,
            evidence_span: None,
            predicate_key: Some("preferred_editor".to_owned()),
            predicate_candidate: None,
            predicate_status: None,
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            polarity: true,
            originator_actor_id: "matthew".to_owned(),
            speaker_actor_id: "matthew".to_owned(),
            transmitter_actor_id: None,
            holder_actor_id: Some("matthew".to_owned()),
            stance: Some(Stance::Endorsing),
            framing_path: Vec::new(),
            confidence: 0.95,
            known_at: Some(Utc::now() - Duration::minutes(1)),
            valid_from: None,
            valid_to: None,
            extractor: "lanternbench".to_owned(),
            extractor_version: "1".to_owned(),
        })
        .await?;
    let old = service
        .reconcile_claim(
            initial.id.as_str(),
            ReconcileClaimRequest {
                independent_evidence: false,
            },
        )
        .await?
        .belief
        .expect("initial belief exists");
    let dependent = service
        .create_belief(BeliefRequest {
            holder_key: "lucy".to_owned(),
            subject_key: "editor_setup".to_owned(),
            predicate_key: "uses_editor".to_owned(),
            current_value: "old plugins".to_owned(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            confidence: 0.7,
            trust_class: Some(TrustClass::Derived),
            known_from: None,
            valid_from: None,
        })
        .await?;
    service
        .store_relation(RelationRequest {
            in_id: old.id.to_string(),
            out_id: dependent.id.to_string(),
            relation_type: "depends_on".to_owned(),
            origin: "lanternbench".to_owned(),
            confidence: 1.0,
            resolved: true,
        })
        .await?;
    let pack = service
        .compile_context(ContextCompileRequest {
            query: "What editor does Matthew use on Mac?".to_owned(),
            actor: Some("lucy".to_owned()),
            project_hints: Vec::new(),
            scope: BTreeMap::from([(String::from("os"), String::from("macos"))]),
            intent: None,
            item_budget: 5,
            token_budget: 2048,
        })
        .await?;
    assert!(pack.selected_ids.contains(&old.id.to_string()));

    let correction = service
        .record_correction(CorrectionRequest {
            target_belief_id: Some(old.id.to_string()),
            target_query: None,
            correction_text: "No, I use Zed on Mac.".to_owned(),
            replacement_value: "Zed".to_owned(),
            context_pack_id: Some(pack.id.to_string()),
            scope: BTreeMap::new(),
        })
        .await?;
    let replacement = correction
        .reconciliation
        .belief
        .expect("correction creates replacement");
    assert_eq!(correction.context_pack_id, Some(pack.id.to_string()));
    assert_eq!(correction.claim.originator_actor_id, "matthew");
    assert_eq!(replacement.current_value, "Zed");
    assert_eq!(
        repository.get_belief(&old.id).await?.unwrap().state,
        BeliefState::Superseded
    );
    assert!(repository.get_belief(&dependent.id).await?.unwrap().stale);

    let explanation = service.explain_belief(replacement.id.as_str()).await?;
    assert!(
        explanation
            .supporting_claims
            .iter()
            .any(|claim| claim.id == correction.claim.id)
    );
    assert!(service.search_beliefs("VS Code", false).await?.is_empty());
    assert_eq!(service.search_beliefs("VS Code", true).await?.len(), 1);
    assert!(
        service
            .belief_history(replacement.id.as_str())
            .await?
            .iter()
            .all(|revision| revision.claim_id.as_ref() == Some(&correction.claim.id))
    );

    let _ = std::fs::remove_dir_all(path);
    Ok(())
}
