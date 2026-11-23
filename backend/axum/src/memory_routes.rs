use crate::{
    auth_middleware::AuthenticatedUser,
    memory_architecture::{
        HandoverContext, HandoverRequest, IndexChatMessageInput, IndexMemoryInput, MemoryEpoch,
        ProgressiveRetrieval, ProgressiveRetrievalRequest,
    },
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
    0.75
}

#[derive(Debug, Serialize)]
pub struct QoinMemoryIndexReceipt {
    pub memory_id: Uuid,
    pub symbol: String,
    pub model_version: String,
    pub indexed: bool,
}

fn qoin_error(error_value: QoinVectorError) -> (StatusCode, Json<ApiError>) {
    match error_value {
        QoinVectorError::MissingVector => error(
            StatusCode::NOT_FOUND,
            "Qoin has no current vector for this symbol.",
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
    if ![
        "agent_private",
        "task_shared",
        "project_shared",
        "workspace_shared",
    ]
    .contains(&input.visibility.as_str())
        || (input.visibility == "agent_private" && input.owner_agent_id.is_none())
        || (input.visibility == "task_shared" && input.task_id.is_none())
        || (input.visibility == "project_shared" && input.project_id.is_none())
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
    let snapshot = client.latest(&input.symbol).await.map_err(qoin_error)?;
    let model_version = snapshot.vector.model_version.clone();
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
