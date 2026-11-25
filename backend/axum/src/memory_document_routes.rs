use crate::{
    auth_middleware::AuthenticatedUser,
    memory_documents::MemoryDocument,
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateMemoryDocumentRequest {
    pub scope: String,
    pub document_type: String,
    pub title: String,
    pub content: String,
    #[serde(default = "default_content_format")]
    pub content_format: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_source_kind")]
    pub source_kind: String,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub owner_agent_id: Option<Uuid>,
    #[serde(default)]
    pub key_metadata: Value,
    #[serde(default)]
    pub value_payload: Value,
    #[serde(default = "default_qkv_version")]
    pub qkv_version: String,
    #[serde(default)]
    pub retention_policy: Option<String>,
}

fn default_content_format() -> String {
    "text/markdown".into()
}
fn default_source() -> String {
    "user-authored".into()
}
fn default_source_kind() -> String {
    "manual_memory".into()
}
fn default_qkv_version() -> String {
    "qkv-metadata-v1".into()
}

#[derive(Debug, Serialize)]
pub struct CreateMemoryDocumentResponse {
    pub memory_id: Uuid,
    pub version: u64,
    pub content_hash: String,
    pub lifecycle_status: &'static str,
    pub sync_status: &'static str,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error: message.into() }))
}

pub async fn create_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateMemoryDocumentRequest>,
) -> Result<(StatusCode, Json<CreateMemoryDocumentResponse>), (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }

    if input.content.trim().is_empty()
        || input.content.len() > 256 * 1024
        || input.title.trim().is_empty()
        || input.title.len() > 300
        || input.tags.len() > 64
        || input.tags.iter().any(|tag| tag.trim().is_empty() || tag.len() > 64)
        || input.document_type.trim().is_empty()
        || input.document_type.len() > 80
    {
        return Err(api_error(StatusCode::BAD_REQUEST, "Memory document fields exceed their validation limits."));
    }

    match input.scope.as_str() {
        "workspace_shared" => {
            if input.task_id.is_some() || input.project_id.is_some() || input.owner_agent_id.is_some() {
                return Err(api_error(StatusCode::BAD_REQUEST, "Workspace-shared memory cannot carry narrower scope identifiers."));
            }
        }
        "task_shared" => {
            let Some(task_id) = input.task_id else {
                return Err(api_error(StatusCode::BAD_REQUEST, "Task-shared memory requires task_id."));
            };
            let task_matches = state.tasks.read().await.get(&task_id)
                .is_some_and(|task| task.workspace_id == workspace_id);
            if !task_matches || input.project_id.is_some() || input.owner_agent_id.is_some() {
                return Err(api_error(StatusCode::NOT_FOUND, "Task scope was not found in this workspace."));
            }
        }
        "agent_private" => {
            let Some(agent_id) = input.owner_agent_id else {
                return Err(api_error(StatusCode::BAD_REQUEST, "Agent-private memory requires owner_agent_id."));
            };
            let agent_matches = state.agents.read().await.get(&agent_id)
                .is_some_and(|agent| agent.workspace_id == workspace_id);
            if !agent_matches || input.task_id.is_some() || input.project_id.is_some() {
                return Err(api_error(StatusCode::NOT_FOUND, "Private-memory agent was not found in this workspace."));
            }
        }
        "project_shared" => {
            return Err(api_error(
                StatusCode::NOT_IMPLEMENTED,
                "Project-shared document access is disabled until Axon has a persisted project-membership authorization model.",
            ));
        }
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "Unsupported memory scope.")),
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory document persistence is unavailable.")
    })?;
    let now = Utc::now();
    let mut document = MemoryDocument {
        memory_id: Uuid::new_v4(),
        workspace_id,
        project_id: input.project_id,
        task_id: input.task_id,
        owner_id: input.owner_agent_id,
        scope: input.scope,
        document_type: input.document_type,
        version: 1,
        title: input.title,
        content: input.content,
        content_format: input.content_format,
        tags: input.tags,
        source: input.source,
        source_kind: input.source_kind,
        source_event_sequence: 0,
        created_at: now,
        updated_at: now,
        content_hash: String::new(),
        key_metadata: input.key_metadata,
        value_payload: input.value_payload,
        qkv_version: input.qkv_version,
        qoin_vector_version: None,
        status: "proposed".into(),
        retention_policy: input.retention_policy,
    };
    document.content_hash = document.calculate_content_hash();
    document.validate().map_err(|_| api_error(StatusCode::BAD_REQUEST, "Memory document is invalid."))?;
    let response = CreateMemoryDocumentResponse {
        memory_id: document.memory_id,
        version: document.version,
        content_hash: document.content_hash.clone(),
        lifecycle_status: "proposed",
        sync_status: "pending",
    };
    outbox.enqueue(&document).await.map_err(|error| {
        tracing::error!(error = %error, "memory document could not be queued");
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document could not be persisted.")
    })?;

    // A successful enqueue is a durable SQLite commit. MongoDB delivery is
    // asynchronous and must not be represented as complete in this response.
    let _ = Duration::ZERO;
    Ok((StatusCode::ACCEPTED, Json(response)))
}
