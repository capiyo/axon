pub struct ModelStatObservation {
    pub completed: bool,
    pub supervisor_passed: bool,
    pub user_retried: bool,
    pub cost_per_run_micros: u64,
    pub latency_ms: u64,
}

/// Update rolling success/retry/cost/latency metrics with a single completed run.
pub fn update_model_stat(
    previous: Option<ModelStat>,
    model_id: Uuid,
    category: &str,
    role: AssignmentRole,
    observation: ModelStatObservation,
) -> ModelStat {
    let now = Utc::now();
    let previous_runs = previous.as_ref().map_or(0, |stat| stat.runs);
    let runs = previous_runs.saturating_add(1);
    let blend = |old: f64, sample: bool| {
        ((old * previous_runs as f64) + if sample { 1.0 } else { 0.0 }) / runs as f64
    };
    ModelStat {
        id: previous.as_ref().map_or_else(Uuid::new_v4, |stat| stat.id),
        model_id,
        category: category.to_owned(),
        role,
        runs,
        completion_rate: blend(
            previous.as_ref().map_or(0.0, |stat| stat.completion_rate),
            observation.completed,
        ),
        supervisor_pass_rate: blend(
            previous
                .as_ref()
                .map_or(0.0, |stat| stat.supervisor_pass_rate),
            observation.supervisor_passed,
        ),
        user_retry_rate: blend(
            previous.as_ref().map_or(0.0, |stat| stat.user_retry_rate),
            observation.user_retried,
        ),
        cost_per_run_micros: previous
            .as_ref()
            .map_or(observation.cost_per_run_micros, |stat| {
                ((u128::from(stat.cost_per_run_micros) * u128::from(previous_runs)
                    + u128::from(observation.cost_per_run_micros))
                    / u128::from(runs))
                .min(u128::from(u64::MAX)) as u64
            }),
        latency_ms: previous.as_ref().map_or(observation.latency_ms, |stat| {
            ((u128::from(stat.latency_ms) * u128::from(previous_runs)
                + u128::from(observation.latency_ms))
                / u128::from(runs))
            .min(u128::from(u64::MAX)) as u64
        }),
        last_updated: now,
    }
}

/// Persist one global registry mutation and its audit event atomically.
pub async fn persist_registry_entity(
    storage: &Arc<dyn Storage>,
    entity: StoredEntity,
    actor_user_id: Uuid,
    event_type: &str,
    entity_type: &str,
    entity_id: &str,
    metadata: serde_json::Value,
) -> Result<(), StorageError> {
    storage
        .apply(StorageMutation {
            entity,
            related_entities: Vec::new(),
            audit: AuditEvent::attributed(
                format!("human:{actor_user_id}"),
                None,
                event_type,
                entity_type,
                entity_id,
                metadata,
            ),
        })
        .await
}

/// Load registry entities from the storage abstraction. Provider/model API
/// credentials are intentionally not represented in registry records.
pub async fn load_registry(
    storage: &Arc<dyn Storage>,
) -> Result<
    (
        Vec<ModelProvider>,
        Vec<ModelRecord>,
        Vec<ModelEvaluation>,
        Vec<ModelStat>,
    ),
    StorageError,
> {
    let providers = storage
        .list_entities(EntityKind::Provider, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Provider(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let models = storage
        .list_entities(EntityKind::Model, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Model(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let evaluations = storage
        .list_entities(EntityKind::ModelEvaluation, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelEvaluation(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let stats = storage
        .list_entities(EntityKind::ModelStat, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelStat(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    Ok((providers, models, evaluations, stats))
}

/// Record an evaluation and automatically exclude an approved model when its
/// category score drops by at least the configured threshold.
pub async fn record_evaluation(
    storage: &Arc<dyn Storage>,
    evaluation: ModelEvaluation,
    score_drop_threshold: f64,
) -> Result<bool, StorageError> {
    if !evaluation.score.is_finite()
        || !(0.0..=1.0).contains(&evaluation.score)
        || evaluation.test_set_version.trim().is_empty()
        || !score_drop_threshold.is_finite()
        || !(0.0..=1.0).contains(&score_drop_threshold)
    {
        return Err(StorageError::InvalidModelEvaluation);
    }
    let previous: Vec<ModelEvaluation> = storage
        .list_entities(EntityKind::ModelEvaluation, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelEvaluation(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .filter(|item| item.model_id == evaluation.model_id && item.category == evaluation.category)
        .collect();
    let latest = previous.iter().max_by_key(|item| item.run_at);
    let dropped = latest.is_some_and(|old| old.score - evaluation.score >= score_drop_threshold);
    let mut related = Vec::new();
    if dropped {
        let mut models: Vec<ModelRecord> = storage
            .list_entities(EntityKind::Model, None)
            .await?
            .into_iter()
            .filter_map(|entity| {
                if let StoredEntity::Model(item) = entity {
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
        if let Some(model) = models
            .iter_mut()
            .find(|item| item.id == evaluation.model_id)
        {
            model.flagged_for_review = true;
            model.healthy = false;
            model.updated_at = Utc::now();
            related.push(StoredEntity::Model(model.clone()));
            related.push(StoredEntity::ModelChangeFlag(ModelChangeFlag {
                id: Uuid::new_v4(),
                model_id: evaluation.model_id,
                change_kind: "score_drop".into(),
                previous_value: latest.map(|item| item.score.to_string()),
                observed_value: Some(evaluation.score.to_string()),
                detected_at: Utc::now(),
                cleared_at: None,
                cleared_by: None,
                resolution_note: None,
            }));
        }
    }
    let event = AuditEvent::attributed(
        "system:model_evaluation",
        None,
        if dropped {
            "model.evaluation_drop_flagged"
        } else {
            "model.evaluation_recorded"
        },
        "model_evaluation",
        evaluation.id.to_string(),
        json!({"model_id": evaluation.model_id, "category": evaluation.category,
            "score": evaluation.score, "test_set_version": evaluation.test_set_version, "score_drop": dropped}),
    );
    storage
        .apply(StorageMutation {
            entity: StoredEntity::ModelEvaluation(evaluation),
            related_entities: related,
            audit: event,
        })
        .await?;
    Ok(dropped)
}

pub fn provider_event_metadata(provider: &ModelProvider) -> serde_json::Value {
    json!({ "provider_id": provider.id, "status": provider.status, "terms_reviewed": provider.terms_reviewed_at.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;

