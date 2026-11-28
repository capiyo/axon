use crate::{
    auth_middleware::AuthenticatedUser,
    memory_architecture::{qkv_attention, AttentionMemory, AttentionQuery, QoinVector},
    memory_documents::MemoryDocument,
    memory_intelligence::{
        retrieve_memories_with_config, IndexedMemory, MemoryQuery, MemoryStatus, MemoryVector,
        MemoryVisibility, QoinScoringConfig, ScoreBreakdown,
    },
    memory_outbox::{MemoryLifecycleAction, MemoryReconciliationReport},
    memory_vector_provider::{
        validate_memory_key, LexicalMemoryVectorProvider, MemoryVectorProvider,
        MemoryVectorProviderError, LEXICAL_MEMORY_VECTOR_VERSION,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
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

#[derive(Debug, Serialize)]
pub struct CreateMemoryDocumentResponse {
    pub memory_id: Uuid,
    pub version: u64,
    pub content_hash: String,
    pub source_event_sequence: u64,
    pub lifecycle_status: &'static str,
    pub sync_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    error: String,
}

async fn require_agent_owner(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
    agent_id: Uuid,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    if state.is_agent_owner(user_id, workspace_id, agent_id).await {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::NOT_FOUND,
            "Agent not found for this user in the workspace.",
        ))
    }
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

