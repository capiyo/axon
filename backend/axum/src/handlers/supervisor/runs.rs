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

