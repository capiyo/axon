fn discovery_audit(
    event_type: &str,
    entity_type: &str,
    id: Uuid,
    metadata: serde_json::Value,
) -> AuditEvent {
    AuditEvent::attributed(
        "system:model_discovery",
        None,
        event_type,
        entity_type,
        id.to_string(),
        metadata,
    )
}

async fn apply(
    storage: &Arc<dyn Storage>,
    entity: StoredEntity,
    related_entities: Vec<StoredEntity>,
    event: AuditEvent,
) -> Result<(), DiscoveryError> {
    storage
        .apply(StorageMutation {
            entity,
            related_entities,
            audit: event,
        })
        .await
        .map_err(|_| DiscoveryError::StorageFailed)
}

pub async fn refresh_provider_catalog(
    storage: &Arc<dyn Storage>,
    source: &dyn ModelCatalogSource,
    provider: &ModelProvider,
) -> Result<DiscoveryReport, DiscoveryError> {
    if !provider.discovery_enabled || provider.status != "active" {
        return Ok(DiscoveryReport::default());
    }
    let discovered = source.discover(provider).await?;
    let prices: BTreeMap<String, CatalogPrice> = source
        .prices(provider)
        .await?
        .into_iter()
        .map(|price| (price.model_name.clone(), price))
        .collect();
    let existing: Vec<ModelRecord> = storage
        .list_entities(EntityKind::Model, None)
        .await
        .map_err(|_| DiscoveryError::StorageFailed)?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Model(model) = entity {
                (model.provider_id == provider.id).then_some(model)
            } else {
                None
            }
        })
        .collect();

    let mut report = DiscoveryReport {
        models_seen: discovered.len(),
        ..DiscoveryReport::default()
    };
    let seen: BTreeSet<String> = discovered
        .iter()
        .map(|item| item.model_name.clone())
        .collect();
    for item in discovered {
        if item.model_name.trim().is_empty()
            || item.categories.is_empty()
            || item.abilities.is_empty()
            || item.context_length == 0
        {
            return Err(DiscoveryError::InvalidCatalog);
        }
        let price = prices.get(&item.model_name);
        let current = existing
            .iter()
            .find(|model| model.model_name == item.model_name);
        if let Some(current) = current {
            if current.flagged_for_review {
                continue;
            }
            let mut updated = current.clone();
            let mut flags = Vec::new();
            if item.retired {
                if !current.flagged_for_review {
                    flags.push(ModelChangeFlag {
                        id: Uuid::new_v4(),
                        model_id: current.id,
                        change_kind: "retired".into(),
                        previous_value: Some("active".into()),
                        observed_value: Some("retired".into()),
                        detected_at: Utc::now(),
                        cleared_at: None,
                        cleared_by: None,
                        resolution_note: None,
                    });
                }
            } else if let Some(price) = price {
                if price.input_cost_per_million != current.input_cost_per_million
                    || price.output_cost_per_million != current.output_cost_per_million
                {
                    flags.push(ModelChangeFlag {
                        id: Uuid::new_v4(),
                        model_id: current.id,
                        change_kind: "price_changed".into(),
                        previous_value: Some(format!(
                            "input={},output={}",
                            current.input_cost_per_million, current.output_cost_per_million
                        )),
                        observed_value: Some(format!(
                            "input={},output={}",
                            price.input_cost_per_million, price.output_cost_per_million
                        )),
                        detected_at: Utc::now(),
                        cleared_at: None,
                        cleared_by: None,
                        resolution_note: None,
                    });
                }
            }
            if item.version.is_some() && item.version != current.version {
                flags.push(ModelChangeFlag {
                    id: Uuid::new_v4(),
                    model_id: current.id,
                    change_kind: "version_changed".into(),
                    previous_value: current.version.clone(),
                    observed_value: item.version.clone(),
                    detected_at: Utc::now(),
                    cleared_at: None,
                    cleared_by: None,
                    resolution_note: None,
                });
            }
            if !flags.is_empty() {
                updated.flagged_for_review = true;
                updated.healthy = false;
                updated.updated_at = Utc::now();
                let related = flags
                    .iter()
                    .cloned()
                    .map(StoredEntity::ModelChangeFlag)
                    .collect();
                let flag_metadata: Vec<_> = flags.iter().map(|flag| {
                    serde_json::json!({"change_kind": flag.change_kind, "flag_id": flag.id})
                }).collect();
                apply(
                    storage,
                    StoredEntity::Model(updated.clone()),
                    related,
                    discovery_audit(
                        "model.change_flagged",
                        "model",
                        current.id,
                        serde_json::json!({"flags": flag_metadata}),
                    ),
                )
                .await?;
                report.changes_flagged += flags.len();
            } else {
                updated.last_seen_at = Utc::now();
                updated.healthy =
                    updated.status == RegistryStatus::Approved && !updated.flagged_for_review;
                updated.updated_at = Utc::now();
                apply(
                    storage,
                    StoredEntity::Model(updated),
                    Vec::new(),
                    discovery_audit(
                        "model.catalog_refreshed",
                        "model",
                        current.id,
                        serde_json::json!({"provider_id": provider.id}),
                    ),
                )
                .await?;
            }
        } else {
            if item.retired {
                continue;
            }
            let price = price.cloned().unwrap_or(CatalogPrice {
                model_name: item.model_name.clone(),
                input_cost_per_million: 0,
                output_cost_per_million: 0,
            });
            let now = Utc::now();
            let model = ModelRecord {
                id: Uuid::new_v4(),
                provider_id: provider.id,
                model_name: item.model_name,
                status: RegistryStatus::Candidate,
                abilities: item.abilities,
                categories: item.categories,
                context_length: item.context_length,
                input_cost_per_million: price.input_cost_per_million,
                output_cost_per_million: price.output_cost_per_million,
                typical_latency_ms: item.typical_latency_ms,
                regions: item.regions,
                version: item.version,
                last_seen_at: now,
                notes: "Discovered; evaluation and explicit admin approval required.".into(),
                curated_rank: BTreeMap::new(),
                healthy: false,
                flagged_for_review: false,
                created_at: now,
                updated_at: now,
            };
            apply(
                storage,
                StoredEntity::Model(model.clone()),
                Vec::new(),
                discovery_audit(
                    "model.candidate_discovered",
                    "model",
                    model.id,
                    serde_json::json!({"provider_id": provider.id, "model_name": model.model_name}),
                ),
            )
            .await?;
            report.candidates_added += 1;
        }
    }

    for current in existing
        .iter()
        .filter(|model| !seen.contains(&model.model_name) && !model.flagged_for_review)
    {
        let mut retired = current.clone();
        retired.flagged_for_review = true;
        retired.healthy = false;
        retired.updated_at = Utc::now();
        let flag = ModelChangeFlag {
            id: Uuid::new_v4(),
            model_id: current.id,
            change_kind: "retired".into(),
            previous_value: Some("listed".into()),
            observed_value: Some("missing_from_catalog".into()),
            detected_at: Utc::now(),
            cleared_at: None,
            cleared_by: None,
            resolution_note: None,
        };
        apply(
            storage,
            StoredEntity::Model(retired),
            vec![StoredEntity::ModelChangeFlag(flag.clone())],
            discovery_audit(
                "model.retirement_flagged",
                "model",
                current.id,
                serde_json::json!({"flag_id": flag.id}),
            ),
        )
        .await?;
        report.changes_flagged += 1;
    }
    Ok(report)
}

