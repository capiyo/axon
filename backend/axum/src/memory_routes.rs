use crate::{
    auth_middleware::AuthenticatedUser,
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
use chrono::Utc;
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
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

#[derive(Debug, Deserialize)]
pub struct AgentSessionContextRequest {
    #[serde(flatten)]
    pub progressive: ProgressiveRetrievalRequest,
    #[serde(default)]
    pub context: Option<f32>,
    #[serde(default)]
    pub outcome: Option<f32>,
    #[serde(default)]
    pub failure: Option<f32>,
    #[serde(default)]
    pub evidence: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct RankedSessionMemoryDocument {
    pub document: MemoryDocument,
    pub qkv_attention_score: f64,
    pub qoin_score: ScoreBreakdown,
    pub qoin_reasons: Vec<String>,
}

fn bounded_session_context(
    candidates: Vec<RankedSessionMemoryDocument>,
    budget_bytes: usize,
) -> Result<(Vec<RankedSessionMemoryDocument>, usize, bool), serde_json::Error> {
    let mut used = 0usize;
    let mut documents = Vec::new();
    let mut truncated = false;
    for candidate in candidates {
        let size = serde_json::to_vec(&candidate)?.len();
        if used.saturating_add(size) > budget_bytes {
            truncated = true;
            continue;
        }
        used += size;
        documents.push(candidate);
    }
    Ok((documents, used, truncated))
}

fn session_memory_metadata_score(document: &MemoryDocument, key: &str) -> f32 {
    document
        .key_metadata
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0) as f32)
        .unwrap_or(0.5)
}

fn session_memory_dependencies(document: &MemoryDocument) -> BTreeSet<Uuid> {
    document
        .key_metadata
        .get("dependency_ids")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect()
}

fn session_memory_outcome_labels(document: &MemoryDocument) -> BTreeSet<String> {
    document
        .key_metadata
        .get("outcome_labels")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn session_memory_visibility(scope: &str) -> Option<MemoryVisibility> {
    match scope {
        "agent_private" => Some(MemoryVisibility::AgentPrivate),
        "task_shared" => Some(MemoryVisibility::TaskShared),
        "project_shared" => Some(MemoryVisibility::ProjectShared),
        "workspace_shared" => Some(MemoryVisibility::WorkspaceShared),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
pub struct AgentSessionMemoryContext {
    pub session_id: Uuid,
    pub epoch_id: Uuid,
    pub progressive: ProgressiveRetrieval,
    pub qkv_model_version: String,
    pub qoin_scoring_version: String,
    pub qoin_scoring_config: QoinScoringConfig,
    pub document_candidates_examined: usize,
    pub document_candidates_ranked: usize,
    pub documents: Vec<RankedSessionMemoryDocument>,
    pub document_context_bytes_used: usize,
    pub documents_truncated: bool,
}

/// Retrieves progressive SQLite memories and ranks the exact MongoDB document
/// versions pinned to the same session epoch through QKV and Qoin.
pub async fn retrieve_session_context(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<AgentSessionContextRequest>,
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

    let fallback_provider = LexicalMemoryVectorProvider;
    let vector_provider: &dyn MemoryVectorProvider = state
        .memory_vector_provider
        .as_deref()
        .unwrap_or(&fallback_provider);
    validate_memory_key(&input.progressive.query, vector_provider.model_version())
        .map_err(|_| error(
            StatusCode::BAD_REQUEST,
            "The session query vector does not match the configured memory embedding provider.",
        ))?;

    // The coordinator verifies the session/agent/workspace binding and records
    // progressive retrieval against the already pinned immutable epoch.
    let progressive = coordinator(&state)?
        .retrieve_progressive(
            workspace_id,
            agent_id,
            session_id,
            input.progressive.clone(),
        )
        .await
        .map_err(|_| error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Could not retrieve additional context from this agent's pinned memory epoch.",
        ))?;

    let query_vector = input.progressive.query.clone();
    let mut attention_candidates = Vec::with_capacity(pinned_documents.len());
    let mut indexed_memories = Vec::with_capacity(pinned_documents.len());
    let mut documents_by_id = HashMap::with_capacity(pinned_documents.len());
    for document in pinned_documents {
        let key = match document.key_metadata.get("qkv_key") {
            Some(stored_key) => serde_json::from_value::<QoinVector>(stored_key.clone())
                .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "A pinned memory QKV key is invalid."))?,
            None => {
                let mut key_text = document.title.clone();
                key_text.push(' ');
                key_text.push_str(&document.content);
                for tag in &document.tags {
                    key_text.push(' ');
                    key_text.push_str(tag);
                }
                vector_provider
                    .key_for_text(&key_text)
                    .await
                    .map_err(|provider_error| match provider_error {
                        MemoryVectorProviderError::EmptyQuery => error(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            "A pinned memory document has no searchable terms.",
                        ),
                        _ => error(StatusCode::SERVICE_UNAVAILABLE, "Memory embedding provider is unavailable."),
                    })?
            }
        };
        validate_memory_key(&key, vector_provider.model_version())
            .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "A pinned memory QKV key has an incompatible model version or vector shape."))?;
        if document.qkv_version != key.model_version
            || key.model_version != query_vector.model_version
        {
            return Err(error(
                StatusCode::CONFLICT,
                "The pinned memory epoch uses a different QKV model version from the configured provider.",
            ));
        }

        let outcome = session_memory_metadata_score(&document, "outcome");
        let evidence = session_memory_metadata_score(&document, "evidence");
        let context = session_memory_metadata_score(&document, "context");
        let failure = session_memory_metadata_score(&document, "failure");
        let dependencies = session_memory_dependencies(&document);
        let visibility = session_memory_visibility(&document.scope)
            .ok_or_else(|| error(StatusCode::UNPROCESSABLE_ENTITY, "A pinned memory document has an unsupported scope."))?;
        let source_sequence = i64::try_from(document.source_event_sequence)
            .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "A pinned memory source sequence is invalid."))?;
        attention_candidates.push(AttentionMemory {
            memory_id: document.memory_id,
            source_kind: document.source_kind.clone(),
            key: key.clone(),
            value: document.memory_id.to_string(),
            outcome_score: outcome,
            evidence_score: evidence,
            source_event_sequence: source_sequence,
            tags: document.tags.iter().cloned().collect(),
            dependency_ids: dependencies.clone(),
        });
        indexed_memories.push(IndexedMemory {
            id: document.memory_id,
            workspace_id,
            chat_id: Uuid::nil(),
            task_id: document.task_id,
            project_id: document.project_id,
            owner_agent_id: document.owner_id,
            visibility,
            status: MemoryStatus::Approved,
            vector: MemoryVector {
                semantic: key.values.clone(),
                context,
                outcome,
                failure,
                evidence,
            },
            tags: document.tags.iter().cloned().collect(),
            dependency_ids: dependencies,
            outcome_labels: session_memory_outcome_labels(&document),
            source_event_ids: Vec::new(),
            created_at_unix: document.created_at.timestamp(),
            superseded_by: None,
        });
        documents_by_id.insert(document.memory_id, document);
    }

    let attention = qkv_attention(
        &AttentionQuery {
            query: query_vector.clone(),
            heads: input.progressive.heads,
            tags: input.progressive.tags.clone(),
            dependency_ids: input.progressive.dependency_ids.clone(),
        },
        &attention_candidates,
    )
    .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "Pinned session QKV retrieval could not be evaluated."))?;
    let attention_scores: HashMap<Uuid, f64> = attention
        .into_iter()
        .map(|score| (score.memory_id, score.total))
        .collect();

    // Omitted Qoin dimensions use neutral midpoints; evidence defaults to 1
    // so the document's recorded evidence strength is not artificially reduced.
    let qoin = retrieve_memories_with_config(
        &MemoryQuery {
            workspace_id,
            requester_agent_id: Some(agent_id),
            task_id,
            project_id: None,
            semantic: query_vector.values,
            context: input.context.unwrap_or(0.5),
            outcome: input.outcome.unwrap_or(0.5),
            failure: input.failure.unwrap_or(0.5),
            evidence: input.evidence.unwrap_or(1.0),
            now_unix: Utc::now().timestamp(),
            tags: input.progressive.tags,
            dependency_ids: input.progressive.dependency_ids,
            outcome_labels: BTreeSet::new(),
            limit: input.progressive.limit.clamp(1, 20),
        },
        &indexed_memories,
        &state.qoin_memory_scoring,
    )
    .map_err(|_| error(StatusCode::BAD_REQUEST, "Qoin session memory ranking could not be evaluated."))?;
    let qoin_scoring_version = qoin.scoring_config.version.clone();
    let qoin_scoring_config = qoin.scoring_config.clone();
    let document_candidates_examined = qoin.candidates_examined;
    let document_candidates_ranked = qoin.ranked.len();
    let mut ranked_documents = Vec::with_capacity(qoin.ranked.len());
    for ranked in qoin.ranked {
        if let Some(document) = documents_by_id.remove(&ranked.memory_id) {
            ranked_documents.push(RankedSessionMemoryDocument {
                document,
                qkv_attention_score: attention_scores.get(&ranked.memory_id).copied().unwrap_or(0.0),
                qoin_score: ranked.score,
                qoin_reasons: ranked.reasons,
            });
        }
    }

    const DOCUMENT_CONTEXT_BUDGET_BYTES: usize = 256 * 1024;
    let (documents, document_context_bytes_used, documents_truncated) =
        bounded_session_context(ranked_documents, DOCUMENT_CONTEXT_BUDGET_BYTES)
            .map_err(|_| error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Ranked session memory context could not be serialized safely.",
            ))?;

    Ok(Json(AgentSessionMemoryContext {
        session_id,
        epoch_id,
        progressive,
        qkv_model_version: query_vector.model_version,
        qoin_scoring_version,
        qoin_scoring_config,
        document_candidates_examined,
        document_candidates_ranked,
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

#[cfg(test)]
mod session_context_tests {
    use super::*;
    use chrono::Utc;

    fn document(content: &str) -> MemoryDocument {
        let now = Utc::now();
        MemoryDocument {
            memory_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            project_id: None,
            task_id: None,
            owner_id: None,
            scope: "workspace_shared".into(),
            document_type: "fact".into(),
            version: 1,
            title: "Test memory".into(),
            content: content.into(),
            content_format: "text/plain".into(),
            tags: Vec::new(),
            source: "test".into(),
            source_kind: "test".into(),
            source_event_sequence: 1,
            created_at: now,
            updated_at: now,
            content_hash: String::new(),
            key_metadata: serde_json::Value::Null,
            value_payload: serde_json::Value::Null,
            qkv_version: "test-qkv-v1".into(),
            qoin_vector_version: None,
            status: "ready".into(),
            retention_policy: None,
        }
    }

    fn ranked_document(content: &str) -> RankedSessionMemoryDocument {
        RankedSessionMemoryDocument {
            document: document(content),
            qkv_attention_score: 0.5,
            qoin_score: ScoreBreakdown {
                semantic: 0.5,
                context: 0.5,
                dependency: 0.0,
                outcome: 0.5,
                failure: 0.5,
                evidence: 0.5,
                recency: 0.5,
                tags: 0.0,
                total: 0.5,
            },
            qoin_reasons: Vec::new(),
        }
    }

    #[test]
    fn context_budget_omits_whole_ranked_documents_and_reports_truncation() {
        let first = ranked_document("short");
        let second = ranked_document(&"x".repeat(1024));
        let first_size = serde_json::to_vec(&first).unwrap().len();
        let (selected, used, truncated) =
            bounded_session_context(vec![first, second], first_size).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(used, first_size);
        assert!(truncated);
    }

    #[test]
    fn context_budget_preserves_all_ranked_documents_that_fit() {
        let first = ranked_document("first");
        let second = ranked_document("second");
        let budget = serde_json::to_vec(&first).unwrap().len()
            + serde_json::to_vec(&second).unwrap().len();
        let (selected, used, truncated) =
            bounded_session_context(vec![first, second], budget).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(used, budget);
        assert!(!truncated);
    }
}
