    fn fixture() -> (Vec<ModelProvider>, Vec<ModelRecord>) {
        let provider_a = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider A".into(),
            base_url: "https://a.invalid".into(),
            model_discovery_url: None,
            pricing_source_url: None,
            api_key_env: None,
            discovery_enabled: false,
            rate_limit_per_minute: 10,
            status: "active".into(),
            data_terms: ProviderDataTerms {
                processing_regions: vec!["eu".into()],
                trains_on_api_inputs: Some(false),
                retention_days: Some(0),
                retention_description: "No retention".into(),
            },
            terms_reviewed_at: Some(Utc::now()),
            reviewed_by: Some(Uuid::new_v4()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let provider_b = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider B".into(),
            ..provider_a.clone()
        };
        let provider_c = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider C".into(),
            ..provider_a.clone()
        };
        let provider_d = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider D".into(),
            ..provider_a.clone()
        };
        let provider_e = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider E".into(),
            ..provider_a.clone()
        };
        let provider_f = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider F".into(),
            ..provider_a.clone()
        };
        let mut abilities = BTreeSet::new();
        abilities.insert("text".into());
        abilities.insert("code".into());
        let mut categories = BTreeSet::new();
        categories.insert("coding".into());
        let model = |provider_id, rank| ModelRecord {
            id: Uuid::new_v4(),
            provider_id,
            model_name: "approved-model".into(),
            status: RegistryStatus::Approved,
            abilities: abilities.clone(),
            categories: categories.clone(),
            context_length: 32_000,
            input_cost_per_million: 1_000_000,
            output_cost_per_million: 1_000_000,
            typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()),
            last_seen_at: Utc::now(),
            notes: String::new(),
            curated_rank: [("coding".to_string(), rank)].into_iter().collect(),
            healthy: true,
            flagged_for_review: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        (
            vec![
                provider_a.clone(),
                provider_b.clone(),
                provider_c.clone(),
                provider_d.clone(),
                provider_e.clone(),
                provider_f.clone(),
            ],
            vec![
                model(provider_a.id, 0),
                model(provider_b.id, 1),
                model(provider_c.id, 2),
                model(provider_d.id, 3),
                model(provider_e.id, 4),
                model(provider_f.id, 5),
            ],
        )
    }

    fn profile(level: TaskLevel) -> AssignmentProfile {
        AssignmentProfile {
            run_id: Uuid::from_u128(5),
            category: "coding".into(),
            level,
            low_risk: false,
            required_abilities: ["text".to_string()].into_iter().collect(),
            required_context_tokens: 8_000,
            privacy: PrivacyRequirement {
                forbid_training: true,
                forbid_retention: true,
                allowed_regions: ["eu".to_string()].into_iter().collect(),
            },
            estimated_input_tokens: 100,
            estimated_output_tokens: 100,
            per_task_budget_micros: 100_000,
            daily_budget_remaining_micros: 100_000,
            exploration_percent: 5,
            minimum_history_runs: 5,
            curated_model_ids: Vec::new(),
        }
    }

    #[test]
    fn rejects_models_missing_required_ability_or_context() {
        let (providers, mut models) = fixture();
        for model in &mut models {
            model.abilities.clear();
            model.context_length = 100;
        }
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn filters_provider_privacy_terms() {
        let (mut providers, models) = fixture();
        providers
            .iter_mut()
            .for_each(|provider| provider.data_terms.trains_on_api_inputs = None);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn important_requires_independent_worker_and_supervisor_providers() {
        let (providers, models) = fixture();
        let plan = assign_models(&profile(TaskLevel::Important), &providers, &models, &[]).unwrap();
        let workers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|item| item.role == AssignmentRole::Worker)
            .collect();
        let supervisors: Vec<_> = plan
            .decisions
            .iter()
            .filter(|item| item.role == AssignmentRole::Supervisor)
            .collect();
        assert_eq!(workers.len(), 2);
        assert_eq!(supervisors.len(), 2);
        assert_ne!(workers[0].chosen_model_id, workers[1].chosen_model_id);
        assert!(supervisors.iter().all(|supervisor| !workers
            .iter()
            .any(|worker| worker.chosen_model_id == supervisor.chosen_model_id)));
    }

    #[test]
    fn cannot_send_unapproved_candidate_to_user_traffic() {
        let (providers, mut models) = fixture();
        models
            .iter_mut()
            .for_each(|model| model.status = RegistryStatus::Candidate);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn important_budget_downgrades_instead_of_exceeding_limit() {
        let (providers, models) = fixture();
        let mut request = profile(TaskLevel::Important);
        request.per_task_budget_micros = 500;
        request.daily_budget_remaining_micros = 500;
        let plan = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(plan.budget_downgraded);
        assert_ne!(plan.level, TaskLevel::Important);
        assert!(plan.estimated_cost_micros <= 500);
    }

    #[test]
    fn history_blends_with_curated_prior_after_minimum_runs() {
        let (providers, models) = fixture();
        let high = ModelStat {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            role: AssignmentRole::Worker,
            runs: 5,
            completion_rate: 1.0,
            supervisor_pass_rate: 1.0,
            user_retry_rate: 0.0,
            cost_per_run_micros: 100,
            latency_ms: 100,
            last_updated: Utc::now(),
        };
        let low_sample = ModelStat {
            runs: 4,
            ..high.clone()
        };
        assert_eq!(
            score_candidate(Some(&low_sample), &TaskLevel::Quick, 5, 2),
            998.0
        );
        let observed = score_candidate(Some(&high), &TaskLevel::Quick, 5, 2);
        assert!(observed > 900.0 && observed < 998.0);
        assert_eq!(providers.len(), 6);
    }

    #[test]
    fn exploration_is_one_quick_low_risk_assignment_only() {
        let (providers, models) = fixture();
        let mut request = profile(TaskLevel::Quick);
        request.low_risk = true;
        request.run_id = Uuid::from_u128(4);
        let plan = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(plan.exploration_used);
        assert_eq!(
            plan.decisions
                .iter()
                .filter(|decision| decision.role == AssignmentRole::Worker)
                .count(),
            1
        );

        request.level = TaskLevel::Important;
        let important = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(!important.exploration_used);
    }

    #[test]
    fn assignment_recovery_keeps_worker_supervisor_diversity_after_timeout() {
        let (providers, models) = fixture();
        let request = profile(TaskLevel::Important);
        let mut plan = assign_models(&request, &providers, &models, &[]).unwrap();
        let failed = plan
            .decisions
            .iter()
            .find(|decision| decision.role == AssignmentRole::Worker)
            .unwrap()
            .chosen_model_id;
        let next =
            recover_assignment(&mut plan, failed, &request, &providers, &models, &[]).unwrap();
        assert_ne!(next, failed);
        let worker_providers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|decision| decision.role == AssignmentRole::Worker)
            .filter_map(|decision| {
                models
                    .iter()
                    .find(|model| model.id == decision.chosen_model_id)
                    .map(|model| model.provider_id)
            })
            .collect();
        let supervisor_providers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|decision| decision.role == AssignmentRole::Supervisor)
            .filter_map(|decision| {
                models
                    .iter()
                    .find(|model| model.id == decision.chosen_model_id)
                    .map(|model| model.provider_id)
            })
            .collect();
        assert!(worker_providers
            .iter()
            .all(|provider| !supervisor_providers.contains(provider)));
    }

    #[test]
    fn fallback_chooses_next_approved_provider_after_timeout() {
        let (providers, models) = fixture();
        let mut excluded = BTreeSet::new();
        excluded.insert(models[0].id);
        let mut forbidden = BTreeSet::new();
        forbidden.insert(models[0].provider_id);
        let next = fallback_model(
            &profile(TaskLevel::Quick),
            &providers,
            &models,
            &[],
            &AssignmentRole::Worker,
            &excluded,
            &forbidden,
        );
        assert!(next.is_some_and(|id| id != models[0].id));
    }

    #[test]
    fn model_statistics_update_after_each_run() {
        let model_id = Uuid::new_v4();
        let first = update_model_stat(
            None,
            model_id,
            "coding",
            AssignmentRole::Worker,
            ModelStatObservation {
                completed: true,
                supervisor_passed: true,
                user_retried: false,
                cost_per_run_micros: 100,
                latency_ms: 200,
            },
        );
        let second = update_model_stat(
            Some(first),
            model_id,
            "coding",
            AssignmentRole::Worker,
            ModelStatObservation {
                completed: false,
                supervisor_passed: false,
                user_retried: true,
                cost_per_run_micros: 300,
                latency_ms: 400,
            },
        );
        assert_eq!(second.runs, 2);
        assert_eq!(second.completion_rate, 0.5);
        assert_eq!(second.supervisor_pass_rate, 0.5);
        assert_eq!(second.user_retry_rate, 0.5);
        assert_eq!(second.cost_per_run_micros, 200);
        assert_eq!(second.latency_ms, 300);
    }

    #[test]
    fn flagged_model_is_excluded_from_assignment() {
        let (providers, mut models) = fixture();
        models
            .iter_mut()
            .for_each(|model| model.flagged_for_review = true);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[tokio::test]
    async fn evaluation_score_drop_flags_and_excludes_approved_model() {
        use crate::{
            audit::AuditEvent,
            storage::{SqliteStorage, StorageMutation},
        };
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let (providers, models) = fixture();
        for entity in [
            StoredEntity::Provider(providers[0].clone()),
            StoredEntity::Model(models[0].clone()),
        ] {
            storage
                .apply(StorageMutation {
                    entity,
                    related_entities: Vec::new(),
                    audit: AuditEvent::unattributed(
                        None,
                        "test.seeded",
                        "test",
                        "seed",
                        serde_json::Value::Null,
                    ),
                })
                .await
                .unwrap();
        }
        let first = ModelEvaluation {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            test_set_version: "v1".into(),
            score: 0.95,
            run_at: Utc::now(),
            cost_micros: 10,
        };
        assert!(!record_evaluation(&storage, first, 0.10).await.unwrap());
        let second = ModelEvaluation {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            test_set_version: "v1".into(),
            score: 0.70,
            run_at: Utc::now() + Duration::seconds(1),
            cost_micros: 12,
        };
        assert!(record_evaluation(&storage, second, 0.10).await.unwrap());
        let stored = storage
            .list_entities(EntityKind::Model, None)
            .await
            .unwrap();
        assert!(
            matches!(&stored[0], StoredEntity::Model(model) if model.flagged_for_review && !model.healthy)
        );
        let flags = storage
            .list_entities(EntityKind::ModelChangeFlag, None)
            .await
            .unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "score_drop")));
    }

    #[test]
    fn diversity_shortfall_degrades_and_is_reported() {
        let (mut providers, mut models) = fixture();
        providers.truncate(1);
        models.truncate(1);
        let plan = assign_models(&profile(TaskLevel::Important), &providers, &models, &[]).unwrap();
        assert_eq!(plan.level, TaskLevel::Quick);
        assert!(plan.diversity_degraded);
        assert!(!plan.notices.is_empty());
    }
}

