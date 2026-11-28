    struct FakeCatalog {
        models: Vec<CatalogModel>,
        prices: Vec<CatalogPrice>,
    }

    #[async_trait]
    impl ModelCatalogSource for FakeCatalog {
        async fn discover(
            &self,
            _provider: &ModelProvider,
        ) -> Result<Vec<CatalogModel>, DiscoveryError> {
            Ok(self.models.clone())
        }
        async fn prices(
            &self,
            _provider: &ModelProvider,
        ) -> Result<Vec<CatalogPrice>, DiscoveryError> {
            Ok(self.prices.clone())
        }
    }

    fn provider() -> ModelProvider {
        ModelProvider {
            id: Uuid::new_v4(),
            name: "Test Provider".into(),
            base_url: "https://provider.test".into(),
            model_discovery_url: Some("https://catalog.test/models.json".into()),
            pricing_source_url: Some("https://catalog.test/prices.json".into()),
            api_key_env: None,
            discovery_enabled: true,
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
        }
    }

    fn catalog_model(name: &str) -> CatalogModel {
        CatalogModel {
            model_name: name.into(),
            abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(),
            context_length: 16_000,
            typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()),
            retired: false,
        }
    }

    async fn save(storage: &Arc<dyn Storage>, entity: StoredEntity) {
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

    #[tokio::test]
    async fn newly_discovered_models_are_candidates_and_never_approved() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let source = FakeCatalog {
            models: vec![catalog_model("new-model")],
            prices: vec![CatalogPrice {
                model_name: "new-model".into(),
                input_cost_per_million: 10,
                output_cost_per_million: 20,
            }],
        };
        let report = refresh_provider_catalog(&storage, &source, &provider)
            .await
            .unwrap();
        assert_eq!(report.candidates_added, 1);
        let models = storage
            .list_entities(EntityKind::Model, None)
            .await
            .unwrap();
        assert!(
            matches!(&models[0], StoredEntity::Model(model) if model.status == RegistryStatus::Candidate && !model.healthy)
        );
    }

    #[tokio::test]
    async fn approved_model_price_changes_are_flagged_without_silent_price_updates() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let now = Utc::now();
        let model = ModelRecord {
            id: Uuid::new_v4(),
            provider_id: provider.id,
            model_name: "existing".into(),
            status: RegistryStatus::Approved,
            abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(),
            context_length: 16_000,
            input_cost_per_million: 10,
            output_cost_per_million: 20,
            typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()),
            last_seen_at: now,
            notes: String::new(),
            curated_rank: BTreeMap::new(),
            healthy: true,
            flagged_for_review: false,
            created_at: now,
            updated_at: now,
        };
        save(&storage, StoredEntity::Model(model.clone())).await;
        let source = FakeCatalog {
            models: vec![catalog_model("existing")],
            prices: vec![CatalogPrice {
                model_name: "existing".into(),
                input_cost_per_million: 99,
                output_cost_per_million: 88,
            }],
        };
        let report = refresh_provider_catalog(&storage, &source, &provider)
            .await
            .unwrap();
        assert_eq!(report.changes_flagged, 1);
        let models = storage
            .list_entities(EntityKind::Model, None)
            .await
            .unwrap();
        assert!(
            matches!(&models[0], StoredEntity::Model(updated) if updated.flagged_for_review && !updated.healthy && updated.input_cost_per_million == 10 && updated.output_cost_per_million == 20)
        );
        let flags = storage
            .list_entities(EntityKind::ModelChangeFlag, None)
            .await
            .unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "price_changed")));
    }

    #[tokio::test]
    async fn model_missing_from_configured_catalog_is_flagged_as_retired() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let now = Utc::now();
        let model = ModelRecord {
            id: Uuid::new_v4(),
            provider_id: provider.id,
            model_name: "retired-model".into(),
            status: RegistryStatus::Approved,
            abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(),
            context_length: 16_000,
            input_cost_per_million: 10,
            output_cost_per_million: 20,
            typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()),
            last_seen_at: now,
            notes: String::new(),
            curated_rank: BTreeMap::new(),
            healthy: true,
            flagged_for_review: false,
            created_at: now,
            updated_at: now,
        };
        save(&storage, StoredEntity::Model(model)).await;
        let source = FakeCatalog {
            models: Vec::new(),
            prices: Vec::new(),
        };
        let report = refresh_provider_catalog(&storage, &source, &provider)
            .await
            .unwrap();
        assert_eq!(report.changes_flagged, 1);
        let flags = storage
            .list_entities(EntityKind::ModelChangeFlag, None)
            .await
            .unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "retired")));
    }
}

