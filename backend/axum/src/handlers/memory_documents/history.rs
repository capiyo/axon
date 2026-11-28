pub struct MemoryDocumentVersionHistoryQuery {
    pub task_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    #[serde(default = "default_version_history_limit")]
    pub limit: usize,
    pub before_version: Option<u64>,
}

fn default_version_history_limit() -> usize {
    20
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentVersionSummary {
    pub memory_id: Uuid,
    pub version: u64,
    pub title: String,
    pub content_hash: String,
    pub source: String,
    pub source_kind: String,
    pub source_event_sequence: u64,
    pub qkv_version: String,
    pub qoin_vector_version: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentVersionHistoryResponse {
    pub memory_id: Uuid,
    pub versions: Vec<MemoryDocumentVersionSummary>,
    pub next_before_version: Option<u64>,
}

pub async fn get_agent_memory_document_versions(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, memory_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<MemoryDocumentVersionHistoryQuery>,
) -> Result<Json<MemoryDocumentVersionHistoryResponse>, (StatusCode, Json<ApiError>)> {
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
    if let Some(task_id) = query.task_id {
        let task_matches = state.tasks.read().await.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        });
        if !task_matches || query.project_id.is_some() {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Task not found for this agent in the workspace.",
            ));
        }
    }
    if let Some(project_id) = query.project_id {
        let project_member = state
            .is_project_member(workspace_id, project_id, user.user_id)
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Project membership could not be checked.",
                )
            })?;
        if !project_member {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Project not found in this workspace.",
            ));
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document metadata is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let limit = query.limit.clamp(1, 100);
    let mut references = outbox
        .list_authorized_document_versions(
            workspace_id,
            memory_id,
            agent_id,
            user.user_id,
            query.task_id,
            query.project_id,
            query.before_version,
            limit + 1,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory version history lookup failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory version history could not be retrieved.",
            )
        })?;
    let has_more = references.len() > limit;
    references.truncate(limit);

    let mut versions = Vec::with_capacity(references.len());
    for reference in &references {
        let document = repository
            .get_version(memory_id, reference.version)
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "MongoDB memory document retrieval failed.",
                )
            })?
            .ok_or_else(|| {
                api_error(
                    StatusCode::CONFLICT,
                    "An authorized memory version is missing from MongoDB.",
                )
            })?;
        if document.validate().is_err()
            || document.workspace_id != workspace_id
            || document.memory_id != memory_id
            || document.version != reference.version
            || document.content_hash != reference.content_hash
            || document.scope != reference.scope
            || document.project_id != reference.project_id
            || document.owner_id != reference.owner_id
            || document.source_event_sequence != reference.source_event_sequence
            || document.qkv_version != reference.qkv_version
            || document.qoin_vector_version != reference.qoin_vector_version
            || document.task_id != reference.task_id
        {
            return Err(api_error(
                StatusCode::CONFLICT,
                "Memory version metadata does not match its SQLite authority.",
            ));
        }
        versions.push(MemoryDocumentVersionSummary {
            memory_id,
            version: document.version,
            title: document.title,
            content_hash: document.content_hash,
            source: document.source,
            source_kind: document.source_kind,
            source_event_sequence: document.source_event_sequence,
            qkv_version: document.qkv_version,
            qoin_vector_version: document.qoin_vector_version,
            created_at: document.created_at,
            updated_at: document.updated_at,
        });
    }
    let next_before_version = if has_more {
        references.last().map(|reference| reference.version)
    } else {
        None
    };
    Ok(Json(MemoryDocumentVersionHistoryResponse {
        memory_id,
        versions,
        next_before_version,
    }))
}

pub async fn get_agent_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, memory_id, version)): Path<(Uuid, Uuid, Uuid, u64)>,
    axum::extract::Query(query): axum::extract::Query<MemoryDocumentTaskQuery>,
) -> Result<Json<MemoryDocument>, (StatusCode, Json<ApiError>)> {
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
    if let Some(task_id) = query.task_id {
        let task_matches = state.tasks.read().await.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        });
        if !task_matches {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Task not found for this agent.",
            ));
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document metadata is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let document = outbox
        .get_authorized_document(
            repository.as_ref(),
            workspace_id,
            memory_id,
            version,
            agent_id,
            user.user_id,
            query.task_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document retrieval failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory document could not be retrieved.",
            )
        })?;
    document
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Memory document not found."))
}

#[derive(Debug, Deserialize)]
pub struct MemoryDocumentSearchRequest {
    pub query: String,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub dependency_ids: BTreeSet<Uuid>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default = "default_half")]
    pub context: f32,
    #[serde(default = "default_half")]
    pub outcome: f32,
    #[serde(default = "default_half")]
    pub failure: f32,
    #[serde(default = "default_half")]
    pub evidence: f32,
}

