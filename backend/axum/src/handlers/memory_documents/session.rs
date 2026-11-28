pub async fn get_agent_session_documents(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<MemoryEpochDocumentResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Workspace access could not be verified.",
            )
        })?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory epoch metadata is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let (epoch_id, task_id, _epoch_qoin_scoring_version, documents) = outbox
        .list_session_authorized_documents(
            repository.as_ref(),
            workspace_id,
            session_id,
            agent_id,
            user.user_id,
            100,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "pinned session memory hydration failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Pinned session memory documents could not be retrieved.",
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Memory session not found for this agent.",
            )
        })?;

    if let Some(task_id) = task_id {
        let task_matches = state.tasks.read().await.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        });
        if !task_matches {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Session task is no longer assigned to this agent.",
            ));
        }
    }
    Ok(Json(MemoryEpochDocumentResponse {
        session_id,
        epoch_id,
        documents,
    }))
}

