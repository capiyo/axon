use crate::{
    auth_middleware::AuthenticatedUser,
    memory_architecture::{
        HandoverContext, HandoverRequest, IndexChatMessageInput, IndexMemoryInput, MemoryEpoch,
        ProgressiveRetrieval, ProgressiveRetrievalRequest,
    },
    memory_documents::MemoryDocument,
    qoin_vectors::{QoinVectorError, QoinVectorSnapshot},
    AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

pub type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;
#[derive(Debug, Serialize)]
pub struct ApiError {
    error: String,
}
fn error(status: StatusCode, message: &str) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}
async fn require_member(state: &AppState, user_id: Uuid, workspace_id: Uuid) -> ApiResult<()> {
    let member = state
        .is_workspace_member(user_id, workspace_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not verify workspace membership.",
            )
        })?;
    if member {
        Ok(())
    } else {
        Err(error(
            StatusCode::FORBIDDEN,
            "Workspace membership is required.",
        ))
    }
}
fn coordinator(state: &AppState) -> ApiResult<&crate::memory_architecture::MemoryEpochCoordinator> {
    state.memory_epochs.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory coordination is unavailable.",
        )
    })
}

#[derive(Debug, Deserialize)]
pub struct QoinVectorQuery {
    pub symbol: String,
}

#[derive(Debug, Deserialize)]
pub struct IndexQoinMemoryRequest {
    pub symbol: String,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub owner_agent_id: Option<Uuid>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub dependency_ids: BTreeSet<Uuid>,
    #[serde(default = "default_outcome_score")]
    pub outcome_score: f32,
    #[serde(default = "default_evidence_score")]
    pub evidence_score: f32,
}

fn default_visibility() -> String {
    "workspace_shared".into()
}
fn default_outcome_score() -> f32 {
    0.5
}
fn default_evidence_score() -> f32 {
    0.5
}

#[derive(Debug, Serialize)]
pub struct QoinMemoryIndexReceipt {
    pub memory_id: Uuid,
    pub symbol: String,
    pub model_version: String,
    pub memory_context_timestamp_ms: i64,
    pub qoin_event_ts_ms: i64,
    pub indexed: bool,
}

fn qoin_error(error_value: QoinVectorError) -> (StatusCode, Json<ApiError>) {
    match error_value {
        QoinVectorError::MissingVector => error(
            StatusCode::NOT_FOUND,
            "Qoin has no historical vector at or before this memory timestamp.",
        ),
        QoinVectorError::InvalidVector | QoinVectorError::MissingModelVersion => error(
            StatusCode::BAD_GATEWAY,
            "Qoin returned an invalid or incompatible vector.",
        ),
        QoinVectorError::HttpStatus(_) | QoinVectorError::Request(_) => error(
            StatusCode::BAD_GATEWAY,
            "Qoin vector service is unavailable.",
        ),
        QoinVectorError::InvalidBaseUrl(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Qoin vector service is not configured correctly.",
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct IndexQoinChatMessageRequest {
    pub symbol: String,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub dependency_ids: BTreeSet<Uuid>,
    #[serde(default = "default_evidence_score")]
    pub evidence_score: f32,
}

#[derive(Debug, Serialize)]
pub struct QoinChatMessageIndexReceipt {
    pub message_id: Uuid,
    pub symbol: String,
    pub model_version: String,
    pub context_timestamp_ms: i64,
    pub qoin_event_ts_ms: i64,
    pub indexed: bool,
}

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
                "Chat message could not be indexed with the historical Qoin vector.",
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
                "Only an approved memory in this chat can be indexed. Check the memory state and Qoin vector metadata.",
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
                "Memory must exist, be approved, and have valid Qoin vector metadata.",
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
    let agents = state.agents.read().await;
    if !agents
        .get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id)
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Agent not found in this workspace.",
        ));
    }
    drop(agents);
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
        .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "Could not construct a verified memory handover. Check the published epoch, Qoin model version, and vector dimensions."))?;
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
    let agents = state.agents.read().await;
    if !agents
        .get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id)
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Agent not found in this workspace.",
        ));
    }
    drop(agents);
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

#[derive(Debug, Serialize)]
pub struct AgentSessionMemoryContext {
    pub session_id: Uuid,
    pub epoch_id: Uuid,
    pub progressive: ProgressiveRetrieval,
    pub documents: Vec<MemoryDocument>,
    pub document_context_bytes_used: usize,
    pub documents_truncated: bool,
}

/// Retrieves both progressive SQLite memories and the immutable MongoDB
/// document versions pinned to the same persisted agent session.
pub async fn retrieve_session_context(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<ProgressiveRetrievalRequest>,
) -> ApiResult<Json<AgentSessionMemoryContext>> {
    require_member(&state, user.user_id, workspace_id).await?;
    let agents = state.agents.read().await;
    if !agents
        .get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id)
    {
        return Err(error(StatusCode::NOT_FOUND, "Agent not found in this workspace."));
    }
    drop(agents);

    // The coordinator verifies the session/agent/workspace binding and records
    // progressive retrieval against the already pinned immutable epoch.
    let progressive = coordinator(&state)?
        .retrieve_progressive(workspace_id, agent_id, session_id, input)
        .await
        .map_err(|_| error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Could not retrieve additional context from this agent's pinned memory epoch.",
        ))?;

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Memory document metadata is unavailable.",
    ))?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| error(
        StatusCode::SERVICE_UNAVAILABLE,
        "MongoDB memory documents are unavailable.",
    ))?;
    let (epoch_id, task_id, pinned_documents) = outbox
        .list_session_authorized_documents(
            repository.as_ref(),
            workspace_id,
            session_id,
            agent_id,
            100,
        )
        .await
        .map_err(|_| error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Pinned session memory documents could not be verified.",
        ))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Memory session not found for this agent."))?;

    if let Some(task_id) = task_id {
        let tasks = state.tasks.read().await;
        if !tasks.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        }) {
            return Err(error(
                StatusCode::FORBIDDEN,
                "The session task is no longer assigned to this agent.",
            ));
        }
    }

    // Bound document context independently from the progressive memory count.
    // Do not truncate individual source documents: omit whole documents and
    // report that the caller can request a narrower retrieval.
    const DOCUMENT_CONTEXT_BUDGET_BYTES: usize = 256 * 1024;
    let mut document_context_bytes_used = 0usize;
    let mut documents = Vec::new();
    let mut documents_truncated = false;
    for document in pinned_documents {
        let document_bytes = document.title.len()
            .saturating_add(document.content.len())
            .saturating_add(document.value_payload.to_string().len());
        if document_context_bytes_used.saturating_add(document_bytes)
            > DOCUMENT_CONTEXT_BUDGET_BYTES
        {
            documents_truncated = true;
            continue;
        }
        document_context_bytes_used += document_bytes;
        documents.push(document);
    }

    Ok(Json(AgentSessionMemoryContext {
        session_id,
        epoch_id,
        progressive,
        documents,
        document_context_bytes_used,
        documents_truncated,
    }))
}

pub async fn index_chat_message(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, chat_id, message_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<IndexChatMessageInput>,
) -> ApiResult<StatusCode> {
    require_member(&state, user.user_id, workspace_id).await?;
    if input.workspace_id != workspace_id
        || input.chat_id != chat_id
        || input.message_id != message_id
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace, chat, and message path identifiers must match the payload.",
        ));
    }
    coordinator(&state)?
        .index_chat_message(input)
        .await
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Chat message must exist and have a valid Qoin vector.",
            )
        })?;
    Ok(StatusCode::CREATED)
}
