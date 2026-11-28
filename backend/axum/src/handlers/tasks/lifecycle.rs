pub async fn create_task(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateTask>,
) -> TaskResult<(StatusCode, Json<Task>)> {
    if !state.workspaces.read().await.contains_key(&workspace_id)
        || !state
            .is_workspace_member(user.user_id, workspace_id)
            .await
            .map_err(|_| storage_failure())?
    {
        return Err(error(StatusCode::NOT_FOUND, "Workspace not found."));
    }

    let title = input.title.trim();
    let objective = input.objective.trim();
    if title.is_empty() || title.len() > 160 || objective.is_empty() || objective.len() > 10_000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Task title must be 1–160 characters and objective 1–10000 characters.",
        ));
    }

    let agents = state.agents.read().await;
    if !agents
        .get(&input.assignee_agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Assignee agent must belong to the target workspace.",
        ));
    }
    drop(agents);

    let artifacts = state.artifacts.read().await;
    if input.context_artifact_ids.len() > 50
        || input.context_artifact_ids.iter().any(|id| {
            !artifacts
                .get(id)
                .is_some_and(|artifact| artifact.workspace_id == workspace_id)
        })
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Every context artifact must exist in this workspace; at most 50 references are allowed.",
        ));
    }
    drop(artifacts);

    if let Some(source_chat_id) = input.source_chat_id {
        let chat_store = state.chat_memory.as_ref().ok_or_else(storage_failure)?;
        if chat_store
            .get_chat_for_user(workspace_id, source_chat_id, user.user_id)
            .await
            .map_err(|_| storage_failure())?
            .is_none()
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Source conversation must belong to the authenticated user and target workspace.",
            ));
        }
    }

    let now = Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        workspace_id,
        created_by: user.user_id,
        source_chat_id: input.source_chat_id,
        title: title.to_owned(),
        objective: objective.to_owned(),
        assignee_agent_id: input.assignee_agent_id,
        context_artifact_ids: input.context_artifact_ids,
        response_artifact_id: None,
        status: TaskStatus::Offered,
        version: 1,
        created_at: now,
        updated_at: now,
        transitions: vec![TaskTransition {
            from: None,
            to: TaskStatus::Offered,
            note: "Task created and offered to the assigned agent.".into(),
            occurred_at: now,
        }],
    };

    state
        .persist_entity(
            StoredEntity::Task(task.clone()),
            Vec::new(),
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(workspace_id),
                "task.created",
                "task",
                task.id.to_string(),
                serde_json::json!({"assignee_agent_id": task.assignee_agent_id, "context_artifact_count": task.context_artifact_ids.len()}),
            ),
        )
        .await
        .map_err(|_| storage_failure())?;
    state.tasks.write().await.insert(task.id, task.clone());
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
) -> TaskResult<Json<Vec<Task>>> {
    if !state.workspaces.read().await.contains_key(&workspace_id)
        || !state
            .is_workspace_member(user.user_id, workspace_id)
            .await
            .map_err(|_| storage_failure())?
    {
        return Err(error(StatusCode::NOT_FOUND, "Workspace not found."));
    }

    let mut tasks: Vec<_> = state
        .tasks
        .read()
        .await
        .values()
        .filter(|task| task.workspace_id == workspace_id)
        .cloned()
        .collect();
    tasks.sort_by_key(|task| task.created_at);
    Ok(Json(tasks))
}

fn allowed_transition(from: &TaskStatus, to: &TaskStatus) -> bool {
    use TaskStatus::*;

    matches!(
        (from, to),
        (Offered, Acknowledged | Rejected | Cancelled)
            | (Acknowledged, InProgress | Cancelled)
            | (InProgress, Submitted | Cancelled)
            | (Submitted, ReviewRequired)
            | (ReviewRequired, Accepted | ChangesRequested)
            | (ChangesRequested, InProgress | Cancelled)
            | (Accepted, Completed)
    )
}

#[derive(Serialize)]
pub struct TaskSubmission {
    pub task: Task,
    pub artifact: Artifact,
}

#[derive(Debug, Default, Deserialize)]
pub struct ExecuteTaskRequest {
    #[serde(default)]
    pub memory_query: Option<QoinVector>,
    #[serde(default)]
    pub heads: usize,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub progressive_limit: usize,
}

