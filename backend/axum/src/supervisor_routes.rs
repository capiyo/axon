use crate::auth_middleware::AuthenticatedUser;
use crate::{
    audit::AuditEvent,
    model_registry::{AssignmentProfile, ModelProvider, ModelRecord, ModelStat, PrivacyRequirement, TaskLevel},
    storage::{EntityKind, StoredEntity},
    supervisor::{
        ChecklistProposal, DeterministicCheck, SupervisorError, SupervisorReview, SupervisorRun,
        SupervisorRunStatus, WorkerResult,
    },
    AppState,
};
use chrono::Utc;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
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
fn default_category() -> String { "general".into() }
fn default_level() -> TaskLevel { TaskLevel::Normal }

fn is_low_risk_quick(level: &TaskLevel, title: &str, objective: &str) -> bool {
    if *level != TaskLevel::Quick || objective.len() > 4_000 { return false; }
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let high_risk_terms = [
        "delete", "drop table", "truncate", "migration", "deploy", "production",
        "security", "authentication", "authorization", "password", "secret",
        "payment", "financial", "medical", "legal", "execute command", "shell",
        "database", "credential", "account access",
    ];
    !high_risk_terms.iter().any(|term| text.contains(term))
}

fn configured_curated_models(category: &str) -> Vec<Uuid> {
    let suffix: String = category.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_uppercase() } else { '_' }).collect();
    let key = format!("AXON_CURATED_MODELS_{suffix}");
    std::env::var(key).unwrap_or_default().split(',')
        .filter_map(|value| Uuid::parse_str(value.trim()).ok())
        .take(6).collect()
}

async fn persist_created_run(
    state: &AppState,
    user: &AuthenticatedUser,
    run: SupervisorRun,
) -> ApiResult<SupervisorRun> {
    let related_entities = run.assignment_decisions.iter().cloned()
        .map(StoredEntity::AssignmentDecision).collect();
    state.persist_entity(
        StoredEntity::SupervisorRun(run.clone()),
        related_entities,
        AuditEvent::attributed(
            format!("human:{}", user.user_id), Some(run.workspace_id),
            "supervisor_run.created", "supervisor_run", run.id.to_string(),
            json!({
                "status": run.status, "assignment_level": run.assignment_level,
                "assignment_decisions": run.assignment_decisions.len(),
                "assignment_notice": run.assignment_notice
            }),
        ),
    ).await.map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Supervisor run could not be persisted."))?;
    state.supervisor_runs.write().await.insert(run.id, run.clone());
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

    let providers: Vec<ModelProvider> = state.list_entities(EntityKind::Provider, None).await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Model providers could not be loaded."))?
        .into_iter().filter_map(|entity| if let StoredEntity::Provider(item) = entity { Some(item) } else { None }).collect();
    let models: Vec<ModelRecord> = state.list_entities(EntityKind::Model, None).await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Model registry could not be loaded."))?
        .into_iter().filter_map(|entity| if let StoredEntity::Model(item) = entity { Some(item) } else { None }).collect();
    let stats: Vec<ModelStat> = state.list_entities(EntityKind::ModelStat, None).await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Model statistics could not be loaded."))?
        .into_iter().filter_map(|entity| if let StoredEntity::ModelStat(item) = entity { Some(item) } else { None }).collect();

    let mut required_abilities = input.required_abilities;
    if required_abilities.is_empty() { required_abilities.insert("text".into()); }
    let objective_tokens = (run.objective.len() as u64 / 4).max(1);
    let privacy = input.privacy.unwrap_or(PrivacyRequirement {
        forbid_training: true, forbid_retention: true, allowed_regions: Default::default(),
    });
    let usage_date = Utc::now().date_naive().to_string();
    let daily_budget_limit = std::env::var("AXON_DAILY_BUDGET_MICROS").ok()
        .and_then(|value| value.parse().ok()).unwrap_or(10_000_000u64);
    let spent_today = state.daily_model_spend(user.user_id, &usage_date).await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Daily model budget could not be loaded."))?;
    let low_risk = is_low_risk_quick(&input.level, &run.title, &run.objective);
    let curated_model_ids = configured_curated_models(&input.category);
    let mut profile = AssignmentProfile {
        run_id: run.id,
        category: if input.category.trim().is_empty() { "general".into() } else { input.category.trim().to_ascii_lowercase() },
        level: input.level,
        low_risk,
        required_abilities,
        required_context_tokens: input.required_context_tokens.max(objective_tokens + 4096),
        privacy,
        estimated_input_tokens: objective_tokens + 4096,
        estimated_output_tokens: 2048,
        per_task_budget_micros: std::env::var("AXON_TASK_BUDGET_MICROS").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000_000),
        daily_budget_remaining_micros: daily_budget_limit.saturating_sub(spent_today),
        exploration_percent: std::env::var("AXON_EXPLORATION_PERCENT").ok().and_then(|v| v.parse().ok()).unwrap_or(5),
        minimum_history_runs: std::env::var("AXON_MIN_MODEL_HISTORY_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(10),
        curated_model_ids,
    };
    let assignment_engine = state.assignment_engine();
    let mut assignment = assignment_engine.assign(&profile, &providers, &models, &stats);
    if let Ok(plan) = &assignment {
        let reserved = state.reserve_model_spend(user.user_id, &usage_date, plan.estimated_cost_micros, daily_budget_limit).await
            .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Daily model budget could not be reserved."))?;
        if !reserved {
            let refreshed_spend = state.daily_model_spend(user.user_id, &usage_date).await
                .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Daily model budget could not be refreshed."))?;
            profile.daily_budget_remaining_micros = daily_budget_limit.saturating_sub(refreshed_spend);
            assignment = assignment_engine.assign(&profile, &providers, &models, &stats);
            if let Ok(retry_plan) = &assignment {
                let retry_reserved = state.reserve_model_spend(user.user_id, &usage_date, retry_plan.estimated_cost_micros, daily_budget_limit).await
                    .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Daily model budget could not be reserved."))?;
                if !retry_reserved {
                    assignment = Err(crate::model_registry::AssignmentError::BudgetExceeded);
                }
            }
        }
    }
    match assignment {
        Ok(plan) => {
            run.assignment_level = Some(plan.level.clone());
            if plan.level == TaskLevel::Quick {
                run.status = SupervisorRunStatus::ReadyForWorkers;
            }
            run.assignment_notice = if plan.notices.is_empty() { None } else { Some(plan.notices.join(" ")) };
            run.assignment_decisions = plan.decisions;
        }
        Err(error) => {
            run.assignment_level = None;
            run.assignment_notice = Some(format!("No AI assignment was made: {error}. This run remains available for manual work."));
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

pub async fn submit_review(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, run_id)): Path<(Uuid, Uuid)>,
    Json(review): Json<SupervisorReview>,
) -> ApiResult<Json<SupervisorRun>> {
    let _guard = state.supervisor_run_lock.lock().await;
    let mut run = find_run(&state, workspace_id, run_id).await?;
    let decision = run.submit_review(review).map_err(supervisor_error)?;
    let event = match decision.map(|item| item.outcome) {
        Some(crate::supervisor::SupervisionOutcome::Accepted) => "supervisor_run.accepted",
        Some(crate::supervisor::SupervisionOutcome::RepairRequired) => {
            "supervisor_run.repair_required"
        }
        Some(crate::supervisor::SupervisionOutcome::Failed) => "supervisor_run.failed",
        None => "supervisor_run.review_submitted",
    };
    Ok(Json(persist_run(&state, &user, run, event).await?))
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
        assert!(is_low_risk_quick(&TaskLevel::Quick, "Summarize", "Summarize this short note."));
        assert!(!is_low_risk_quick(&TaskLevel::Normal, "Summarize", "Summarize this short note."));
        assert!(!is_low_risk_quick(&TaskLevel::Quick, "Deploy", "Deploy this change to production."));
        assert!(!is_low_risk_quick(&TaskLevel::Quick, "Review", &"x".repeat(4_001)));
    }
}
