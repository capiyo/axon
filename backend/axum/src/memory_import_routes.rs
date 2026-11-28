use crate::{
    auth_middleware::AuthenticatedUser,
    memory::{
        validate_import, validate_memory, CreateMemoryRequest, ImportConversationRequest,
        MemoryRecord, MemoryRepository, MemoryStatus, ReviewMemoryRequest,
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
    validate_import(&input).map_err(|message| error(StatusCode::BAD_REQUEST, message))?;
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
    validate_memory(&input).map_err(|message| error(StatusCode::BAD_REQUEST, message))?;
    let record = repository(&state)?
        .create_memory(workspace_id, input)
        .await
        .map_err(|cause| {
            let message = cause.to_string();
            let status = if message.contains("Source conversation import was not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            error(status, &message)
        })?;
    Ok((StatusCode::CREATED, Json(record)))
}

#[derive(Debug, Deserialize)]
pub struct MemoryContextQuery {
    pub status: Option<MemoryStatus>,
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
            query.status,
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

pub async fn get_import(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, import_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<crate::memory::ConversationImport>> {
    require_member(&state, &user, workspace_id).await?;
    let record = repository(&state)?
        .get_import(workspace_id, import_id)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not retrieve conversation import.",
            )
        })?;
    record.map(Json).ok_or_else(|| {
        error(
            StatusCode::NOT_FOUND,
            "Conversation import was not found in this workspace.",
        )
    })
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
            query.limit.unwrap_or(20).clamp(1, 50),
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
