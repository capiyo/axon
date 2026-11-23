use crate::{
    auth_middleware::AuthenticatedUser,
    memory_store::{MemoryStoreError},
    memory_system::{
        MemoryRecord, MemoryScope, MemorySensitivity, RetrievalContext, Verification,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

#[derive(Debug, serde::Serialize)]
struct ApiError {
    error: String,
}

type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error: message.into() }))
}

fn store_error(err: MemoryStoreError) -> (StatusCode, Json<ApiError>) {
    match err {
        MemoryStoreError::NotFound => error(StatusCode::NOT_FOUND, "Memory record not found."),
        MemoryStoreError::Policy(_) => error(StatusCode::BAD_REQUEST, "Memory record failed validation."),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Memory operation failed."),
    }
}

async fn require_member(
    state: &AppState,
    user: &AuthenticatedUser,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let member = state.is_workspace_member(user.user_id, workspace_id).await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be checked."))?;
    if !member {
        return Err(error(StatusCode::FORBIDDEN, "Workspace membership is required."));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct ProposeMemory {
    pub scope: MemoryScope,
    pub scope_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub content: String,
    pub source: String,
    pub sensitivity: MemorySensitivity,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

pub async fn propose(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<ProposeMemory>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    require_member(&state, &user, workspace_id).await?;
    if input.content.trim().is_empty() || input.content.len() > 100_000
        || input.source.trim().is_empty() || input.source.len() > 2_000

    {
        return Err(error(StatusCode::BAD_REQUEST, "Memory content or provenance is invalid."));
    }
    if let Some(agent_id) = input.owner_agent_id {
        let agents = state.agents.read().await;
        if !agents.get(&agent_id).is_some_and(|agent| agent.workspace_id == workspace_id) {
            return Err(error(StatusCode::BAD_REQUEST, "Agent scope must reference an agent in this workspace."));
        }
    }
    if let Some(task_id) = input.scope_id.filter(|_| matches!(input.scope, MemoryScope::Task | MemoryScope::Execution)) {
        let tasks = state.tasks.read().await;
        if !tasks.get(&task_id).is_some_and(|task| task.workspace_id == workspace_id) {
            return Err(error(StatusCode::BAD_REQUEST, "Task scope must reference a task in this workspace."));
        }
    }
    let token_cost = ((input.content.chars().count() + 3) / 4).max(1);
    let record = MemoryRecord {
        id: Uuid::new_v4(),
        workspace_id,
        scope: input.scope,
        scope_id: input.scope_id,
        owner_agent_id: input.owner_agent_id,
        content: input.content,
        source: format!("human:{}; {}", user.user_id, input.source),
        confidence: 0.5,
        authority: 0.5,
        sensitivity: input.sensitivity,
        // Agent and user writes are proposals. Only a separate trusted review
        // operation may promote a record to Verified.
        verification: Verification::Proposed,
        created_at: Utc::now(),
        expires_at: input.expires_at,
        version: 1,
        token_cost,
    };
    let store = state.memory_store.as_ref().ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Persistent memory is unavailable."))?;
    store.put(record.clone(), &format!("human:{}", user.user_id)).await.map_err(store_error)?;
    Ok((StatusCode::CREATED, Json(json!({"memory_id": record.id, "verification": "proposed", "version": 1}))))
}

#[derive(Debug, Deserialize)]
pub struct ReviewMemory {
    pub decision: String,
}

pub async fn review(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ReviewMemory>,
) -> ApiResult<Json<MemoryRecord>> {
    require_member(&state, &user, workspace_id).await?;
    let approved = match input.decision.as_str() {
        "approve" => true,
        "reject" => false,
        _ => return Err(error(StatusCode::BAD_REQUEST, "Decision must be approve or reject.")),
    };
    let store = state.memory_store.as_ref().ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Persistent memory is unavailable."))?;
    let record = store.review(workspace_id, memory_id, approved, &format!("human:{}", user.user_id)).await.map_err(store_error)?;
    Ok(Json(record))
}

#[derive(Debug, Deserialize)]
pub struct ShareMemory {
    pub agent_id: Uuid,
}

pub async fn share(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ShareMemory>,
) -> ApiResult<StatusCode> {
    require_member(&state, &user, workspace_id).await?;
    let agents = state.agents.read().await;
    if !agents.get(&input.agent_id).is_some_and(|agent| agent.workspace_id == workspace_id) {
        return Err(error(StatusCode::BAD_REQUEST, "Recipient agent must belong to this workspace."));
    }
    drop(agents);
    let store = state.memory_store.as_ref().ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Persistent memory is unavailable."))?;
    store.grant(workspace_id, memory_id, input.agent_id, &format!("human:{}", user.user_id)).await.map_err(store_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, agent_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_member(&state, &user, workspace_id).await?;
    let agents = state.agents.read().await;
    if !agents.get(&agent_id).is_some_and(|agent| agent.workspace_id == workspace_id) {
        return Err(error(StatusCode::BAD_REQUEST, "Recipient agent must belong to this workspace."));
    }
    drop(agents);
    let store = state.memory_store.as_ref().ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Persistent memory is unavailable."))?;
    store.revoke(workspace_id, memory_id, agent_id, &format!("human:{}", user.user_id)).await.map_err(store_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct AllocateMemory {
    pub task_id: Option<Uuid>,
    pub agent_id: Uuid,
    pub token_budget: usize,
    pub query: String,
}

pub async fn allocate(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<AllocateMemory>,
) -> ApiResult<Json<crate::memory_system::MemoryAllocation>> {
    require_member(&state, &user, workspace_id).await?;
    if input.token_budget == 0 || input.token_budget > 2_000_000 || input.query.trim().is_empty() || input.query.len() > 20_000 {
        return Err(error(StatusCode::BAD_REQUEST, "Context token budget is outside the supported range."));
    }
    let agents = state.agents.read().await;
    if !agents.get(&input.agent_id).is_some_and(|agent| agent.workspace_id == workspace_id) {
        return Err(error(StatusCode::BAD_REQUEST, "Agent must belong to this workspace."));
    }
    drop(agents);
    if let Some(task_id) = input.task_id {
        let tasks = state.tasks.read().await;
        if !tasks.get(&task_id).is_some_and(|task| task.workspace_id == workspace_id && task.assignee_agent_id == input.agent_id) {
            return Err(error(StatusCode::FORBIDDEN, "Only the task's assigned agent may receive task-scoped memory."));
        }
    }
    let context = RetrievalContext {
        workspace_id,
        task_id: input.task_id,
        agent_id: input.agent_id,
        // Current member permissions authorize internal memory only. Higher
        // sensitivity requires a dedicated role grant, not a request flag.
        allowed_sensitivity: MemorySensitivity::Internal,
        granted_memory_ids: BTreeSet::new(),
        now: Utc::now(),
    };
    let store = state.memory_store.as_ref().ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Persistent memory is unavailable."))?;
    let allocation = store.allocate(&context, &input.query, input.token_budget).await.map_err(store_error)?;
    Ok(Json(allocation))
}
