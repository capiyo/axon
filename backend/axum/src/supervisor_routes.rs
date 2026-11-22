use crate::auth_middleware::AuthenticatedUser;
use crate::{
    audit::AuditEvent,
    storage::StoredEntity,
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
    let run =
        SupervisorRun::new(workspace_id, input.title, input.objective).map_err(supervisor_error)?;
    let run = persist_run(&state, &user, run, "supervisor_run.created").await?;
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
    runs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
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
