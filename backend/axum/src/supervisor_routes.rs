use crate::auth_middleware::AuthenticatedUser;
use crate::{
    audit::AuditEvent,
    model_registry::{
        AssignmentProfile, ModelProvider, ModelRecord, ModelStat, PrivacyRequirement, TaskLevel,
    },
    storage::{EntityKind, StoredEntity},
    supervisor::{
        ChecklistProposal, DeterministicCheck, SupervisorError, SupervisorReview, SupervisorRun,
        SupervisorRunStatus, WorkerResult,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;

#[derive(Debug, Serialize)]
pub struct ApiError {
    error: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSupervisorRun {
    pub title: String,
    pub objective: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_level")]
    pub level: TaskLevel,
    #[serde(default)]
    pub required_abilities: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub required_context_tokens: u64,
    #[serde(default)]
    pub privacy: Option<PrivacyRequirement>,
}
fn default_category() -> String {
    "general".into()
}
fn default_level() -> TaskLevel {
    TaskLevel::Normal
}

fn is_low_risk_quick(level: &TaskLevel, title: &str, objective: &str) -> bool {
    if *level != TaskLevel::Quick || objective.len() > 4_000 {
        return false;
    }
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let high_risk_terms = [
        "delete",
        "drop table",
        "truncate",
        "migration",
        "deploy",
        "production",
        "security",
        "authentication",
        "authorization",
        "password",
        "secret",
        "payment",
        "financial",
        "medical",
        "legal",
        "execute command",
        "shell",
        "database",
        "credential",
        "account access",
    ];
    !high_risk_terms.iter().any(|term| text.contains(term))
}


fn task_risk_score(title: &str, objective: &str) -> f64 {
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let high_risk_terms = [
        "delete", "drop table", "truncate", "migration", "deploy", "production",
        "security", "authentication", "authorization", "password", "secret",
        "payment", "financial", "medical", "legal", "shell", "database",
        "credential", "account access",
    ];
    if high_risk_terms.iter().any(|term| text.contains(term)) {
        0.95
    } else {
        0.25
    }
}

fn task_ambiguity(title: &str, objective: &str) -> f64 {
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let vague_terms = [
        "somehow",
        "maybe",
        "some stuff",
        "etc",
        "as needed",
        "make it better",
        "figure it out",
    ];
    let vague_count = vague_terms.iter().filter(|term| text.contains(**term)).count();
    if vague_count >= 2 || objective.len() > 8_000 {
        0.85
    } else if vague_count == 1 || objective.len() > 4_000 {
        0.65
    } else {
        0.20
    }
}

fn configured_curated_models(category: &str) -> Vec<Uuid> {
    let suffix: String = category
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let key = format!("AXON_CURATED_MODELS_{suffix}");
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| Uuid::parse_str(value.trim()).ok())
        .take(6)
        .collect()
}

async fn persist_created_run(
    state: &AppState,
    user: &AuthenticatedUser,
    run: SupervisorRun,
) -> ApiResult<SupervisorRun> {
    let related_entities = run
        .assignment_decisions
        .iter()
        .cloned()
        .map(StoredEntity::AssignmentDecision)
        .collect();
    state
        .persist_entity(
            StoredEntity::SupervisorRun(run.clone()),
            related_entities,
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(run.workspace_id),
                "supervisor_run.created",
                "supervisor_run",
                run.id.to_string(),
                json!({
                    "status": run.status, "assignment_level": run.assignment_level,
                    "assignment_decisions": run.assignment_decisions.len(),
                    "assignment_notice": run.assignment_notice
                }),
            ),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Supervisor run could not be persisted.",
            )
        })?;
    state
        .supervisor_runs
        .write()
        .await
        .insert(run.id, run.clone());
    Ok(run)
}

#[derive(Debug, Deserialize)]
pub struct RecordChecks {
    pub checks: Vec<DeterministicCheck>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

fn supervisor_error(error: SupervisorError) -> (StatusCode, Json<ApiError>) {
    let status = match &error {
        SupervisorError::InvalidRunState
        | SupervisorError::DuplicateSubmission
        | SupervisorError::RepairNotAvailable => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    api_error(status, error.to_string())
}

async fn persist_run(
    state: &AppState,
    user: &AuthenticatedUser,
    run: SupervisorRun,
    event_type: &'static str,
) -> ApiResult<SupervisorRun> {
    state
        .persist_entity(
            StoredEntity::SupervisorRun(run.clone()),
            Vec::new(),
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(run.workspace_id),
                event_type,
                "supervisor_run",
                run.id.to_string(),
                json!({
                    "status": run.status,
                    "version": run.version,
                    "repair_rounds_used": run.repair_rounds_used
                }),
            ),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Supervisor run could not be persisted.",
            )
        })?;
    state
        .supervisor_runs
        .write()
        .await
        .insert(run.id, run.clone());
    Ok(run)
}

async fn find_run(
    state: &AppState,
    workspace_id: Uuid,
    run_id: Uuid,
) -> Result<SupervisorRun, (StatusCode, Json<ApiError>)> {
    let runs = state.supervisor_runs.read().await;
    runs.get(&run_id)
        .filter(|run| run.workspace_id == workspace_id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Supervisor run not found."))
}

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
    let policy_escalated = (risk >= 0.80 || ambiguity >= 0.75)
        && profile.level != TaskLevel::Important;
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
        && assignment.as_ref().is_ok_and(|plan| plan.level != TaskLevel::Important)
    {
        // Do not silently trade away mandatory supervision to satisfy a budget.
        assignment = Err(crate::model_registry::AssignmentError::BudgetExceeded);
    }
    let has_alternative = assignment.as_ref().is_ok_and(|plan| {
        plan.decisions.iter().any(|decision| decision.candidates_considered.len() > 1)
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
        },
        crate::vector_decision::AgentAction::Delegate => {
            Some("Vector policy recommends delegation.".to_string())
        },
        crate::vector_decision::AgentAction::Split => Some(
            "Vector policy requires task decomposition in the checklist before workers proceed."
                .to_string(),
        ),
        crate::vector_decision::AgentAction::RequestEvidence => {
            Some("Vector policy requires more evidence before execution.".to_string())
        },
        crate::vector_decision::AgentAction::Escalate => {
            Some("Vector policy escalated this task to higher supervision.".to_string())
        },
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
            run.assignment_notice = if notices.is_empty() { None } else { Some(notices.join(" ")) };
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

pub async fn list_runs(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<SupervisorRun>>> {
    if !state.workspaces.read().await.contains_key(&workspace_id) {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let mut runs: Vec<_> = state
        .supervisor_runs
        .read()
        .await
        .values()
        .filter(|run| run.workspace_id == workspace_id)
        .cloned()
        .collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.updated_at));
    Ok(Json(runs))
}

pub async fn get_run(
    State(state): State<AppState>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<SupervisorRun>> {
    let runs = state.supervisor_runs.read().await;
    let run = runs
        .get(&run_id)
        .filter(|run| run.workspace_id == workspace_id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Supervisor run not found."))?;
    Ok(Json(run))
}

pub async fn submit_checklist(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
    Json(proposal): Json<ChecklistProposal>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    run.submit_checklist(proposal).map_err(supervisor_error)?;
    let event = if run.status == SupervisorRunStatus::ReadyForWorkers {
        "supervisor_run.checklists_merged"
    } else {
        "supervisor_run.checklist_submitted"
    };
    Ok(Json(persist_run(&state, &user, run, event).await?))
}

pub async fn submit_worker_result(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
    Json(result): Json<WorkerResult>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    run.submit_worker_result(result).map_err(supervisor_error)?;
    let event = if run.status == SupervisorRunStatus::AwaitingChecks {
        "supervisor_run.workers_completed"
    } else {
        "supervisor_run.worker_submitted"
    };
    Ok(Json(persist_run(&state, &user, run, event).await?))
}

pub async fn record_checks(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<RecordChecks>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    run.record_checks(input.checks).map_err(supervisor_error)?;
    Ok(Json(
        persist_run(&state, &user, run, "supervisor_run.checks_recorded").await?,
    ))
}

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
        let Some(mut stat) = existing_stat else {
            // Cold-start models retain their curated prior until an actual
            // evaluation or enough observed history exists.
            continue;
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
                        "runs": stat.runs
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

#[cfg(test)]
mod assignment_policy_tests {
    use super::*;

    #[test]
    fn exploration_is_limited_to_deterministically_low_risk_quick_tasks() {
        assert!(is_low_risk_quick(
            &TaskLevel::Quick,
            "Summarize",
            "Summarize this short note."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Normal,
            "Summarize",
            "Summarize this short note."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Quick,
            "Deploy",
            "Deploy this change to production."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Quick,
            "Review",
            &"x".repeat(4_001)
        ));
    }
}

#[cfg(test)]
mod vector_policy_tests {
    use super::*;

    #[test]
    fn production_and_credential_tasks_are_high_risk() {
        assert!(task_risk_score("Deploy", "Deploy to production") >= 0.80);
        assert!(task_risk_score("Auth", "Rotate the API credential") >= 0.80);
        assert!(task_risk_score("Notes", "Summarize this paragraph") < 0.80);
    }

    #[test]
    fn vague_objectives_are_flagged_for_decomposition() {
        assert!(task_ambiguity(
            "Build feature",
            "Figure it out somehow and handle the rest"
        ) >= 0.75);
        assert!(task_ambiguity("Summarize", "Summarize the supplied paragraph") < 0.75);
    }
}
