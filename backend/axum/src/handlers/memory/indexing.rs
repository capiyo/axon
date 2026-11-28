pub async fn index_chat_message_with_qoin(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, chat_id, message_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<IndexQoinChatMessageRequest>,
) -> ApiResult<Json<QoinChatMessageIndexReceipt>> {
    require_member(&state, user.user_id, workspace_id).await?;
    let client = state.qoin_vectors.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Qoin integration is not configured. Set QOIN_API_URL on the Axon backend.",
        )
    })?;
    let context_timestamp_ms = coordinator(&state)?
        .chat_message_context_timestamp_ms(workspace_id, chat_id, message_id)
        .await
        .map_err(|_| error(StatusCode::NOT_FOUND, "Source chat message was not found."))?;
    let snapshot = client
        .at(&input.symbol, context_timestamp_ms)
        .await
        .map_err(qoin_error)?;
    let model_version = snapshot.vector.model_version.clone();
    let qoin_event_ts_ms = snapshot.event_ts_ms.ok_or_else(|| {
        error(
            StatusCode::BAD_GATEWAY,
            "Qoin historical response did not include its source event timestamp.",
        )
    })?;
    coordinator(&state)?
        .index_chat_message(IndexChatMessageInput {
            workspace_id,
            chat_id,
            message_id,
            vector: snapshot.vector,
            tags: input.tags,
            dependency_ids: input.dependency_ids,
            evidence_score: input.evidence_score,
        })
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Chat message could not be indexed with the historical vector snapshot.",
            )
        })?;
    Ok(Json(QoinChatMessageIndexReceipt {
        message_id,
        symbol: snapshot.symbol,
        model_version,
        context_timestamp_ms,
        qoin_event_ts_ms,
        indexed: true,
    }))
}

pub async fn latest_qoin_vector(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<QoinVectorQuery>,
) -> ApiResult<Json<QoinVectorSnapshot>> {
    require_member(&state, user.user_id, workspace_id).await?;
    let client = state.qoin_vectors.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Qoin integration is not configured. Set QOIN_API_URL on the Axon backend.",
        )
    })?;
    let snapshot = client.latest(&query.symbol).await.map_err(qoin_error)?;
    Ok(Json(snapshot))
}

pub async fn index_memory_with_qoin(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, chat_id, memory_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<IndexQoinMemoryRequest>,
) -> ApiResult<Json<QoinMemoryIndexReceipt>> {
    require_member(&state, user.user_id, workspace_id).await?;
    if !["agent_private", "task_shared", "workspace_shared"].contains(&input.visibility.as_str())
        || (input.visibility == "agent_private" && input.owner_agent_id.is_none())
        || (input.visibility == "task_shared" && input.task_id.is_none())
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Invalid memory visibility or missing scope identifier.",
        ));
    }
    if let Some(agent_id) = input.owner_agent_id {
        let agents = state.agents.read().await;
        if !agents
            .get(&agent_id)
            .is_some_and(|agent| agent.workspace_id == workspace_id)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Private-memory owner is not registered in this workspace.",
            ));
        }
    }
    if let Some(task_id) = input.task_id {
        let tasks = state.tasks.read().await;
        if !tasks
            .get(&task_id)
            .is_some_and(|task| task.workspace_id == workspace_id)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Task scope does not belong to this workspace.",
            ));
        }
    }
    let client = state.qoin_vectors.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Qoin integration is not configured. Set QOIN_API_URL on the Axon backend.",
        )
    })?;
    let memory_context_timestamp_ms = coordinator(&state)?
        .approved_memory_context_timestamp_ms(workspace_id, chat_id, memory_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Only an approved memory with an original creation timestamp can be indexed.",
            )
        })?;
    let snapshot = client
        .at(&input.symbol, memory_context_timestamp_ms)
        .await
        .map_err(qoin_error)?;
    let model_version = snapshot.vector.model_version.clone();
    let qoin_event_ts_ms = snapshot.event_ts_ms.ok_or_else(|| {
        error(
            StatusCode::BAD_GATEWAY,
            "Qoin historical response did not include its source event timestamp.",
        )
    })?;
    coordinator(&state)?
        .index_approved_memory(IndexMemoryInput {
            workspace_id,
            chat_id,
            memory_id,
            task_id: input.task_id,
            project_id: input.project_id,
            owner_agent_id: input.owner_agent_id,
            visibility: input.visibility,
            vector: snapshot.vector,
            tags: input.tags,
            dependency_ids: input.dependency_ids,
            outcome_score: input.outcome_score,
            evidence_score: input.evidence_score,
        })
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Only an approved memory in this chat can be indexed. Check the memory state and vector metadata.",
            )
        })?;
    Ok(Json(QoinMemoryIndexReceipt {
        memory_id,
        symbol: snapshot.symbol,
        model_version,
        memory_context_timestamp_ms,
        qoin_event_ts_ms,
        indexed: true,
    }))
}

pub async fn index_memory(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<IndexMemoryInput>,
) -> ApiResult<StatusCode> {
    require_member(&state, user.user_id, workspace_id).await?;
    if input.workspace_id != workspace_id {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace path and payload must match.",
        ));
    }
    if let Some(agent_id) = input.owner_agent_id {
        let agents = state.agents.read().await;
        if !agents
            .get(&agent_id)
            .is_some_and(|agent| agent.workspace_id == workspace_id)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Private-memory owner is not registered in this workspace.",
            ));
        }
    }
    if let Some(task_id) = input.task_id {
        let tasks = state.tasks.read().await;
        if !tasks
            .get(&task_id)
            .is_some_and(|task| task.workspace_id == workspace_id)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Task scope does not belong to this workspace.",
            ));
        }
    }
    coordinator(&state)?
        .index_approved_memory(input)
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Memory must exist, be approved, and have valid vector metadata.",
            )
        })?;
    Ok(StatusCode::CREATED)
}

pub async fn prepare_handover(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<HandoverRequest>,
) -> ApiResult<Json<HandoverContext>> {
    require_member(&state, user.user_id, workspace_id).await?;
    if input.workspace_id != workspace_id || input.agent_id != agent_id {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace and agent path identifiers must match the payload.",
        ));
    }
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    if let Some(task_id) = input.task_id {
        let tasks = state.tasks.read().await;
        if !tasks.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        }) {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Task is not assigned to this agent in this workspace.",
            ));
        }
    }
    let context = coordinator(&state)?.prepare_handover(input).await
        .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "Could not construct a verified memory handover. Check the published epoch, compatible vector model version, and vector dimensions."))?;
    Ok(Json(context))
}

pub async fn latest_epoch(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Option<MemoryEpoch>>> {
    require_member(&state, user.user_id, workspace_id).await?;
    let epoch = coordinator(&state)?
        .latest_epoch(workspace_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load memory epoch.",
            )
        })?;
    Ok(Json(epoch))
}

pub async fn progressive_retrieval(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<ProgressiveRetrievalRequest>,
) -> ApiResult<Json<ProgressiveRetrieval>> {
    require_member(&state, user.user_id, workspace_id).await?;
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    let result = coordinator(&state)?
        .retrieve_progressive(workspace_id, agent_id, session_id, input)
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Could not retrieve additional context from this agent's pinned memory epoch.",
            )
        })?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
