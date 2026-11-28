pub async fn submit_task(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(task_id): Path<Uuid>,
    Json(input): Json<SubmitTask>,
) -> TaskResult<Json<TaskSubmission>> {
    let title = input.title.trim();
    let content = input.content.trim();
    if title.is_empty() || title.len() > 200 || content.is_empty() || content.len() > 512_000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Response title must be 1–200 characters and response content 1–512000 bytes.",
        ));
    }

    let workspace_id = state
        .tasks
        .read()
        .await
        .get(&task_id)
        .map(|task| task.workspace_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Task not found."))?;
    if !state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| storage_failure())?
    {
        return Err(error(StatusCode::NOT_FOUND, "Task not found."));
    }

    let mut tasks = state.tasks.write().await;
    let Some(mut task) = tasks.get(&task_id).cloned() else {
        return Err(error(StatusCode::NOT_FOUND, "Task not found."));
    };
    if task.version != input.expected_version {
        return Err((
            StatusCode::CONFLICT,
            Json(TaskApiError {
                error: "Task version conflict. Reload the task before retrying.".into(),
                current_version: Some(task.version),
            }),
        ));
    }
    if task.status != TaskStatus::InProgress {
        return Err(error(
            StatusCode::CONFLICT,
            "A response can only be submitted while the task is in progress.",
        ));
    }

    let now = Utc::now();
    let artifact = Artifact {
        id: Uuid::new_v4(),
        workspace_id: task.workspace_id,
        title: title.to_owned(),
        content: content.to_owned(),
        source_agent_id: Some(task.assignee_agent_id),
        captured_at: now,
    };
    task.response_artifact_id = Some(artifact.id);
    task.status = TaskStatus::Submitted;
    task.version += 1;
    task.updated_at = now;
    task.transitions.push(TaskTransition {
        from: Some(TaskStatus::InProgress),
        to: TaskStatus::Submitted,
        note: format!(
            "Response captured as artifact {artifact_id}.",
            artifact_id = artifact.id
        ),
        occurred_at: now,
    });

    state
        .persist_entity(
            StoredEntity::Task(task.clone()),
            vec![StoredEntity::Artifact(artifact.clone())],
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(task.workspace_id),
                "task.response_submitted",
                "task",
                task.id.to_string(),
                serde_json::json!({"response_artifact_id": artifact.id, "version": task.version}),
            ),
        )
        .await
        .map_err(|_| storage_failure())?;

    state
        .artifacts
        .write()
        .await
        .insert(artifact.id, artifact.clone());
    tasks.insert(task.id, task.clone());

    Ok(Json(TaskSubmission { task, artifact }))
}

pub async fn transition_task(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(task_id): Path<Uuid>,
    Json(input): Json<TransitionTask>,
) -> TaskResult<Json<Task>> {
    let note = input.note.trim();
    if note.is_empty() || note.len() > 2_000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "A transition note of 1–2000 characters is required.",
        ));
    }

    let workspace_id = state
        .tasks
        .read()
        .await
        .get(&task_id)
        .map(|task| task.workspace_id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Task not found."))?;
    if !state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| storage_failure())?
    {
        return Err(error(StatusCode::NOT_FOUND, "Task not found."));
    }

    let mut tasks = state.tasks.write().await;
    let Some(mut task) = tasks.get(&task_id).cloned() else {
        return Err(error(StatusCode::NOT_FOUND, "Task not found."));
    };

    if task.version != input.expected_version {
        return Err((
            StatusCode::CONFLICT,
            Json(TaskApiError {
                error: "Task version conflict. Reload the task before retrying.".into(),
                current_version: Some(task.version),
            }),
        ));
    }

    if !allowed_transition(&task.status, &input.status) {
        return Err(error(
            StatusCode::CONFLICT,
            format!(
                "Transition from {from:?} to {to:?} is not allowed.",
                from = task.status,
                to = input.status
            ),
        ));
    }

    let previous = task.status.clone();
    let now = Utc::now();
    task.status = input.status.clone();
    task.version += 1;
    task.updated_at = now;
    task.transitions.push(TaskTransition {
        from: Some(previous),
        to: input.status,
        note: note.to_owned(),
        occurred_at: now,
    });

    state
        .persist_entity(
            StoredEntity::Task(task.clone()),
            Vec::new(),
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(task.workspace_id),
                "task.transitioned",
                "task",
                task.id.to_string(),
                serde_json::json!({"to": task.status, "version": task.version}),
            ),
        )
        .await
        .map_err(|_| storage_failure())?;
    tasks.insert(task.id, task.clone());
    Ok(Json(task))
}

