use crate::{
    auth_middleware::AuthenticatedUser,
    memory::{
        CreateMemoryRequest, ImportConversationRequest, MemoryRecord, MemoryRepository,
        MemoryStatus, ReviewMemoryRequest,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn error(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message })))
}

async fn require_member(
    state: &AppState,
    user: &AuthenticatedUser,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Workspace access could not be checked.",
            )
        })?;
    if member {
        Ok(())
    } else {
        Err(error(StatusCode::NOT_FOUND, "Workspace not found."))
    }
}

fn repository(state: &AppState) -> ApiResult<&MemoryRepository> {
    state.memory.as_deref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Durable memory storage is unavailable. Configure MongoDB.",
        )
    })
}

pub async fn import_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<ImportConversationRequest>,
) -> ApiResult<(StatusCode, Json<crate::memory::ConversationImport>)> {
    require_member(&state, &user, workspace_id).await?;
    if !["chatgpt", "claude"].contains(&input.provider.trim().to_ascii_lowercase().as_str())
        || input.provider.len() > 32
        || input.conversation_id.trim().is_empty()
        || input.conversation_id.len() > 200
        || input.title.trim().is_empty()
        || input.title.len() > 160
        || input.messages.is_empty()
        || input.messages.len() > 2_000
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Conversation import metadata or message count is invalid.",
        ));
    }
    let mut total_bytes = 0usize;
    for message in &input.messages {
        if !["user", "assistant", "system"].contains(&message.role.as_str())
            || message.content.trim().is_empty()
            || message.content.len() > 256_000
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Conversation message role or content is invalid.",
            ));
        }
        total_bytes = total_bytes.saturating_add(message.content.len());
        if total_bytes > 512_000 {
            return Err(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Conversation import exceeds 512000 bytes.",
            ));
        }
    }
    let imported = repository(&state)?
        .import_conversation(workspace_id, input)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not import conversation.",
            )
        })?;
    Ok((StatusCode::CREATED, Json(imported)))
}

pub async fn create_memory(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateMemoryRequest>,
) -> ApiResult<(StatusCode, Json<MemoryRecord>)> {
    require_member(&state, &user, workspace_id).await?;
    if input.title.trim().is_empty()
        || input.title.len() > 240
        || input.content.trim().is_empty()
        || input.content.len() > 20_000
        || input.summary.trim().is_empty()
        || input.summary.len() > 2_000
        || input.tags.len() > 30
        || input
            .tags
            .iter()
            .any(|tag| tag.trim().is_empty() || tag.len() > 60)
        || !input.confidence.is_finite()
        || !(0.0..=1.0).contains(&input.confidence)
        || input.message_ids.len() > 200
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Proposed memory fields exceed supported limits.",
        ));
    }
    let record = repository(&state)?
        .create_memory(workspace_id, input)
        .await
        .map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "Memory source import or message references are invalid.",
            )
        })?;
    Ok((StatusCode::CREATED, Json(record)))
}

#[derive(Debug, Deserialize)]
pub struct MemoryContextQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_memories(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<MemoryContextQuery>,
) -> ApiResult<Json<Vec<MemoryRecord>>> {
    require_member(&state, &user, workspace_id).await?;
    if query.q.as_ref().is_some_and(|value| value.len() > 120) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Memory search query must not exceed 120 characters.",
        ));
    }
    let records = repository(&state)?
        .list_memories(
            workspace_id,
            None,
            query.q.as_deref(),
            query.limit.unwrap_or(50).clamp(1, 200),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not list workspace memories.",
            )
        })?;
    Ok(Json(records))
}

pub async fn memory_context(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<MemoryContextQuery>,
) -> ApiResult<Json<Vec<MemoryRecord>>> {
    require_member(&state, &user, workspace_id).await?;
    if query.q.as_ref().is_some_and(|value| value.len() > 120) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Memory search query must not exceed 120 characters.",
        ));
    }
    let records = repository(&state)?
        .list_memories(
            workspace_id,
            Some(MemoryStatus::Active),
            query.q.as_deref(),
            query.limit.unwrap_or(10).clamp(1, 50),
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not retrieve active memory context.",
            )
        })?;
    Ok(Json(records))
}

pub async fn review_memory(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ReviewMemoryRequest>,
) -> ApiResult<Json<MemoryRecord>> {
    require_member(&state, &user, workspace_id).await?;
    if input.expected_version == 0 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Expected memory version must be positive.",
        ));
    }
    let record = repository(&state)?
        .review_memory(workspace_id, memory_id, input)
        .await
        .map_err(|_| {
            error(
                StatusCode::CONFLICT,
                "Memory review conflicted with a newer version or invalid decision.",
            )
        })?;
    record.map(Json).ok_or_else(|| {
        error(
            StatusCode::NOT_FOUND,
            "Proposed memory was not found in this workspace.",
        )
    })
}
