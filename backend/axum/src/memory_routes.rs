use crate::{
    auth_middleware::AuthenticatedUser,
    memory_architecture::{
        HandoverContext, HandoverRequest, IndexChatMessageInput, IndexMemoryInput, MemoryEpoch,
        ProgressiveRetrieval, ProgressiveRetrievalRequest,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
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
