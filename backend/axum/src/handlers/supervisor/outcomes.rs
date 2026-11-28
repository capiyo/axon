async fn update_model_outcomes(
    state: &AppState,
    user: &AuthenticatedUser,
    run: &SupervisorRun,
    accepted: bool,
) -> ApiResult<()> {
    let Some(category) = run.assignment_category.as_deref() else {
        return Ok(());
    };
    let decisions = state
        .list_entities(EntityKind::AssignmentDecision, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Assignment outcomes could not be loaded.",
            )
        })?;
    let stats = state
        .list_entities(EntityKind::ModelStat, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model statistics could not be loaded.",
            )
        })?;
    let models = state
        .list_entities(EntityKind::Model, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model registry could not be loaded.",
            )
        })?;
    let mut updated_keys = Vec::new();
    for entity in decisions {
        let StoredEntity::AssignmentDecision(decision) = entity else {
            continue;
        };
        if decision.run_id != run.id
            || updated_keys.iter().any(|(model_id, role)| {
                *model_id == decision.chosen_model_id && role == &decision.role
            })
        {
            continue;
        }
        let existing_stat = stats.iter().find_map(|entity| match entity {
            StoredEntity::ModelStat(item)
                if item.model_id == decision.chosen_model_id
                    && item.category == category
                    && item.role == decision.role =>
            {
                Some(item.clone())
            }
            _ => None,
        });
        let mut stat = match existing_stat {
            Some(stat) => stat,
            None => {
                let Some(model) = models.iter().find_map(|entity| match entity {
                    StoredEntity::Model(item) if item.id == decision.chosen_model_id => {
                        Some(item.clone())
                    }
                    _ => None,
                }) else {
                    continue;
                };
                ModelStat {
                    id: Uuid::new_v4(),
                    model_id: decision.chosen_model_id,
                    category: category.to_string(),
                    role: decision.role.clone(),
                    runs: 0,
                    completion_rate: 0.5,
                    supervisor_pass_rate: 0.5,
                    // Neutral until an actual user-retry signal is available.
                    user_retry_rate: 0.5,
                    cost_per_run_micros: decision.estimated_cost_micros,
                    latency_ms: model.typical_latency_ms,
                    last_updated: Utc::now(),
                }
            }
        };
        let next_runs = stat.runs.saturating_add(1);
        if next_runs == stat.runs {
            continue;
        }
        let prior_runs = stat.runs as f64;
        let total_runs = next_runs as f64;
        let outcome = if accepted { 1.0 } else { 0.0 };
        stat.completion_rate =
            ((stat.completion_rate.clamp(0.0, 1.0) * prior_runs) + outcome) / total_runs;
        stat.supervisor_pass_rate =
            ((stat.supervisor_pass_rate.clamp(0.0, 1.0) * prior_runs) + outcome) / total_runs;
        // Retry rate, provider latency and actual billed cost are deliberately
        // untouched: this workflow does not observe those measurements.
        stat.runs = next_runs;
        stat.last_updated = Utc::now();
        state
            .persist_entity(
                StoredEntity::ModelStat(stat.clone()),
                Vec::new(),
                AuditEvent::attributed(
                    format!("human:{}", user.user_id),
                    Some(run.workspace_id),
                    "model_stat.supervisor_outcome_recorded",
                    "model_stat",
                    stat.id.to_string(),
                    json!({
                        "run_id": run.id,
                        "model_id": stat.model_id,
                        "category": category,
                        "accepted": accepted,
                        "runs": stat.runs,
                        "cost_latency_source": "registry_estimate",
                        "user_retry_rate_observed": false
                    }),
                ),
            )
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Model outcome statistics could not be persisted.",
                )
            })?;
        updated_keys.push((decision.chosen_model_id, decision.role.clone()));
    }
    Ok(())
}

pub async fn submit_review(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
    Json(review): Json<SupervisorReview>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    let decision = run.submit_review(review).map_err(supervisor_error)?;
    let outcome = decision.as_ref().map(|item| item.outcome.clone());
    let event = match outcome.as_ref() {
        Some(crate::supervisor::SupervisionOutcome::Accepted) => "supervisor_run.accepted",
        Some(crate::supervisor::SupervisionOutcome::RepairRequired) => {
            "supervisor_run.repair_required"
        }
        Some(crate::supervisor::SupervisionOutcome::Failed) => "supervisor_run.failed",
        None => "supervisor_run.review_submitted",
    };
    let persisted = persist_run(&state, &user, run, event).await?;
    match outcome.as_ref() {
        Some(crate::supervisor::SupervisionOutcome::Accepted) => {
            update_model_outcomes(&state, &user, &persisted, true).await?;
        }
        Some(crate::supervisor::SupervisionOutcome::Failed) => {
            update_model_outcomes(&state, &user, &persisted, false).await?;
        }
        _ => {}
    }
    Ok(Json(persisted))
}

pub async fn start_repair(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    run.start_repair().map_err(supervisor_error)?;
    Ok(Json(
        persist_run(&state, &user, run, "supervisor_run.repair_started").await?,
    ))
}

