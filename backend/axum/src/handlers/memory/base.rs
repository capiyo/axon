use crate::{
    auth_middleware::{AuthenticatedAgent, AuthenticatedUser},
    memory_architecture::{
        qkv_attention, AttentionMemory, AttentionQuery, HandoverContext, HandoverRequest,
        IndexChatMessageInput, IndexMemoryInput, MemoryEpoch, ProgressiveRetrieval,
        ProgressiveRetrievalRequest, QoinVector,
    },
    memory_documents::MemoryDocument,
    memory_intelligence::{
        retrieve_memories_with_config, IndexedMemory, MemoryQuery, MemoryStatus, MemoryVector,
        MemoryVisibility, QoinScoringConfig, ScoreBreakdown,
    },
    memory_vector_provider::{
        validate_memory_key, LexicalMemoryVectorProvider, MemoryVectorProvider,
        MemoryVectorProviderError,
    },
    qoin_vectors::{QoinVectorError, QoinVectorSnapshot},
    AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
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
async fn require_agent_owner(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
    agent_id: Uuid,
) -> ApiResult<()> {
    if state.is_agent_owner(user_id, workspace_id, agent_id).await {
        Ok(())
    } else {
        Err(error(
            StatusCode::NOT_FOUND,
            "Agent not found for this user in the workspace.",
        ))
    }
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

