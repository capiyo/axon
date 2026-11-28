pub async fn create_run(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateSupervisorRun>,
) -> ApiResult<(StatusCode, Json<SupervisorRun>)> {
    if input.title.trim().is_empty()
        || input.title.trim().len() > 200
        || input.objective.trim().is_empty()
        || input.objective.len() > 20_000
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Supervisor run title must be 1–200 characters and objective 1–20000 characters.",
        ));
    }
    if !state.workspaces.read().await.contains_key(&workspace_id) {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let mut run =
        SupervisorRun::new(workspace_id, input.title, input.objective).map_err(supervisor_error)?;

    let providers: Vec<ModelProvider> = state
        .list_entities(EntityKind::Provider, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model providers could not be loaded.",
            )
        })?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Provider(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let models: Vec<ModelRecord> = state
        .list_entities(EntityKind::Model, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model registry could not be loaded.",
            )
        })?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Model(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let stats: Vec<ModelStat> = state
        .list_entities(EntityKind::ModelStat, None)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model statistics could not be loaded.",
            )
        })?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelStat(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();

    let mut required_abilities = input.required_abilities;
    if required_abilities.is_empty() {
        required_abilities.insert("text".into());
    }
    let objective_tokens = (run.objective.len() as u64 / 4).max(1);
    let privacy = input.privacy.unwrap_or(PrivacyRequirement {
        forbid_training: true,
        forbid_retention: true,
        allowed_regions: Default::default(),
    });
    let usage_date = Utc::now().date_naive().to_string();
    let daily_budget_limit = std::env::var("AXON_DAILY_BUDGET_MICROS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000_000u64);
    let spent_today = state
        .daily_model_spend(user.user_id, &usage_date)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Daily model budget could not be loaded.",
            )
        })?;
    let low_risk = is_low_risk_quick(&input.level, &run.title, &run.objective);
    let curated_model_ids = configured_curated_models(&input.category);
    let mut profile = AssignmentProfile {
        run_id: run.id,
        category: if input.category.trim().is_empty() {
            "general".into()
        } else {
            input.category.trim().to_ascii_lowercase()
        },
        level: input.level,
        low_risk,
        required_abilities,
        required_context_tokens: input.required_context_tokens.max(objective_tokens + 4096),
        privacy,
        estimated_input_tokens: objective_tokens + 4096,
        estimated_output_tokens: 2048,
        per_task_budget_micros: std::env::var("AXON_TASK_BUDGET_MICROS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2_000_000),
        daily_budget_remaining_micros: daily_budget_limit.saturating_sub(spent_today),
        exploration_percent: std::env::var("AXON_EXPLORATION_PERCENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5),
        minimum_history_runs: std::env::var("AXON_MIN_MODEL_HISTORY_RUNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        curated_model_ids,
    };
    run.assignment_category = Some(profile.category.clone());
    let risk = task_risk_score(&run.title, &run.objective);
    let ambiguity = task_ambiguity(&run.title, &run.objective);
    let policy_escalated =
        (risk >= 0.80 || ambiguity >= 0.75) && profile.level != TaskLevel::Important;
    if policy_escalated {
        // Quick tasks with material risk or ambiguity require the checklist and
        // independent review stages rather than bypassing supervision.
        profile.level = TaskLevel::Important;
    }
    let assignment_engine = state.assignment_engine();
    let mut assignment = assignment_engine.assign(&profile, &providers, &models, &stats);
    if let Ok(plan) = &assignment {
        let reserved = state
            .reserve_model_spend(
                user.user_id,
                workspace_id,
                &usage_date,
                plan.estimated_cost_micros,
                daily_budget_limit,
            )
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Daily model budget could not be reserved.",
                )
            })?;
        if !reserved {
            let refreshed_spend = state
                .daily_model_spend(user.user_id, &usage_date)
                .await
                .map_err(|_| {
                    api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Daily model budget could not be refreshed.",
                    )
                })?;
            profile.daily_budget_remaining_micros =
                daily_budget_limit.saturating_sub(refreshed_spend);
            assignment = assignment_engine.assign(&profile, &providers, &models, &stats);
            if let Ok(retry_plan) = &assignment {
                let retry_reserved = state
                    .reserve_model_spend(
                        user.user_id,
                        workspace_id,
                        &usage_date,
                        retry_plan.estimated_cost_micros,
                        daily_budget_limit,
                    )
                    .await
                    .map_err(|_| {
                        api_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Daily model budget could not be reserved.",
                        )
                    })?;
                if !retry_reserved {
                    assignment = Err(crate::model_registry::AssignmentError::BudgetExceeded);
                }
            }
        }
    }
    if policy_escalated
        && assignment
            .as_ref()
            .is_ok_and(|plan| plan.level != TaskLevel::Important)
    {
        // Do not silently trade away mandatory supervision to satisfy a budget.
        assignment = Err(crate::model_registry::AssignmentError::BudgetExceeded);
    }
    let has_alternative = assignment.as_ref().is_ok_and(|plan| {
        plan.decisions
            .iter()
            .any(|decision| decision.candidates_considered.len() > 1)
    });
    let capability_fit = assignment
        .as_ref()
        .ok()
        .map(|plan| {
            let scores = plan
                .decisions
                .iter()
                .filter_map(|decision| {
                    let model = models
                        .iter()
                        .find(|model| model.id == decision.chosen_model_id)?;
                    let stat = stats.iter().find(|stat| {
                        stat.model_id == decision.chosen_model_id
                            && stat.category == profile.category
                            && stat.role == decision.role
                    });
                    let curated_rank = profile
                        .curated_model_ids
                        .iter()
                        .position(|id| *id == decision.chosen_model_id)
                        .map(|rank| rank as u16)
                        .or_else(|| model.curated_rank.get(&profile.category).copied())
                        .unwrap_or(0);
                    Some(
                        crate::model_registry::score_candidate(
                            stat,
                            &plan.level,
                            profile.minimum_history_runs,
                            curated_rank,
                        ) / 1000.0,
                    )
                })
                .collect::<Vec<_>>();
            if scores.is_empty() {
                0.10
            } else {
                scores.iter().sum::<f64>() / scores.len() as f64
            }
        })
        .unwrap_or(0.10);
    let action = crate::vector_decision::VectorDecisionEngine.choose_action(
        crate::vector_decision::DecisionContext {
            capability_fit,
            evidence_completeness: 1.0,
            ambiguity,
            risk,
            eligible_alternative_exists: has_alternative,
            can_split: ambiguity >= 0.75,
        },
        crate::vector_decision::DecisionThresholds::default(),
    );
    let action_notice = match action {
        crate::vector_decision::AgentAction::Execute => {
            Some("Vector policy selected execution.".to_string())
        }
        crate::vector_decision::AgentAction::Delegate => {
            Some("Vector policy recommends delegation.".to_string())
        }
        crate::vector_decision::AgentAction::Split => Some(
            "Vector policy requires task decomposition in the checklist before workers proceed."
                .to_string(),
        ),
        crate::vector_decision::AgentAction::RequestEvidence => {
            Some("Vector policy requires more evidence before execution.".to_string())
        }
        crate::vector_decision::AgentAction::Escalate => {
            Some("Vector policy escalated this task to higher supervision.".to_string())
        }
        crate::vector_decision::AgentAction::Abstain => Some(
            "Vector policy abstained because no eligible agent assignment was available."
                .to_string(),
        ),
    };
    match assignment {
        Ok(plan) => {
            run.assignment_level = Some(plan.level.clone());
            if plan.level == TaskLevel::Quick {
                run.status = SupervisorRunStatus::ReadyForWorkers;
            }
            let mut notices = plan.notices;
            if policy_escalated {
                notices.push(
                    "Vector policy upgraded the requested level to Important due to risk or ambiguity."
                        .into(),
                );
            }
            if let Some(notice) = action_notice.clone() {
                notices.push(notice);
            }
            run.assignment_notice = if notices.is_empty() {
                None
            } else {
                Some(notices.join(" "))
            };
            run.assignment_decisions = plan.decisions;
        }
        Err(error) => {
            run.assignment_level = None;
            let mut notice = format!(
                "No AI assignment was made: {error}. This run remains available for manual work."
            );
            if let Some(action_notice) = action_notice {
                notice.push(' ');
                notice.push_str(&action_notice);
            }
            if policy_escalated {
                notice.push_str(
                    " Vector policy requires Important supervision due to risk or ambiguity.",
                );
            }
            run.assignment_notice = Some(notice);
        }
    }
    let run = persist_created_run(&state, &user, run).await?;
    Ok((StatusCode::CREATED, Json(run)))
}

