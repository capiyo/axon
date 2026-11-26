use crate::{
    auth_middleware::AuthenticatedUser,
    memory_architecture::{qkv_attention, AttentionMemory, AttentionQuery, QoinVector},
    memory_documents::MemoryDocument,
    memory_outbox::MemoryReconciliationReport,
    memory_vector_provider::{LexicalMemoryVectorProvider, MemoryVectorProvider, MemoryVectorProviderError, LEXICAL_MEMORY_VECTOR_VERSION},
    memory_intelligence::{
        retrieve_memories_with_config, IndexedMemory, MemoryQuery, MemoryStatus, MemoryVector,
        MemoryVisibility, QoinScoringConfig, ScoreBreakdown,
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
use std::{collections::{BTreeSet, HashMap}, sync::Arc};
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
    let mut key_text = input.title.clone();
    key_text.push(' ');
    key_text.push_str(&input.content);
    for tag in &input.tags {
        key_text.push(' ');
        key_text.push_str(tag);
    }
    let fallback_provider = LexicalMemoryVectorProvider;
    let vector_provider: &dyn MemoryVectorProvider = state
        .memory_vector_provider
        .as_deref()
        .unwrap_or(&fallback_provider);
    let qkv_key = vector_provider
        .key_for_text(&key_text)
        .await
        .map_err(|provider_error| match provider_error {
            MemoryVectorProviderError::EmptyQuery => {
                api_error(StatusCode::BAD_REQUEST, "Memory document contains no searchable terms.")
            }
            _ => api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory embedding provider is unavailable."),
        })?;
    if qkv_key.model_version != vector_provider.model_version() {
        return Err(api_error(StatusCode::BAD_GATEWAY, "Memory embedding provider returned an incompatible model version."));
    }
    let mut key_metadata = match input.key_metadata {
        Value::Object(metadata) => metadata,
        Value::Null => Map::new(),
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "key_metadata must be a JSON object.")),
    };
    let serialized_key = serde_json::to_value(&qkv_key)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "QKV key could not be serialized."))?;
    key_metadata.insert("qkv_key".into(), serialized_key);
    key_metadata.insert("qkv_key_version".into(), Value::String(qkv_key.model_version.clone()));

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
        key_metadata: Value::Object(key_metadata),
        value_payload: input.value_payload,
        qkv_version: qkv_key.model_version.clone(),
        qoin_vector_version: None,
        status: "proposed".into(),
        retention_policy: input.retention_policy,
    };
    document.content_hash = document.calculate_content_hash();
    let qkv_source_hash = document.calculate_qkv_source_hash();
    if let Value::Object(metadata) = &mut document.key_metadata {
        metadata.insert("qkv_source_hash".into(), Value::String(qkv_source_hash));
    }
    document.validate().map_err(|_| api_error(StatusCode::BAD_REQUEST, "Memory document is invalid."))?;
    outbox.enqueue(&mut document, user.user_id).await.map_err(|error| {
        tracing::error!(error = %error, "memory document could not be queued");
        api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document could not be persisted.")
    })?;
    let response = CreateMemoryDocumentResponse {
        memory_id: document.memory_id,
        version: document.version,
        content_hash: document.content_hash.clone(),
        source_event_sequence: document.source_event_sequence,
        lifecycle_status: "proposed",
        sync_status: "pending",
    };

    // A successful enqueue is a durable SQLite commit. MongoDB delivery is
    // asynchronous and must not be represented as complete in this response.
    Ok((StatusCode::ACCEPTED, Json(response)))
}


#[derive(Debug, Deserialize)]
pub struct MemoryDocumentTaskQuery {
    pub task_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentApprovalResponse {
    pub memory_id: Uuid,
    pub version: u64,
    pub lifecycle_status: &'static str,
}

pub async fn approve_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentApprovalResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory document storage is unavailable.")
    })?;
    let approved = outbox
        .approve(workspace_id, memory_id, version, user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document approval failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document approval failed.")
        })?;
    if !approved {
        return Err(api_error(StatusCode::NOT_FOUND, "Proposed memory document was not found."));
    }
    Ok(Json(MemoryDocumentApprovalResponse {
        memory_id,
        version,
        lifecycle_status: "approved",
    }))
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentRetryResponse {
    pub memory_id: Uuid,
    pub version: u64,
    pub sync_status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct MemoryReconciliationQuery {
    #[serde(default = "default_reconciliation_limit")]
    pub limit: usize,
}

fn default_reconciliation_limit() -> usize {
    1000
}

pub async fn reconcile_memory_documents(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<MemoryReconciliationQuery>,
) -> Result<Json<MemoryReconciliationReport>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory reconciliation is unavailable.")
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "MongoDB memory documents are unavailable.")
    })?;
    let report = outbox
        .reconcile_workspace(repository.as_ref(), workspace_id, query.limit)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory reconciliation failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory reconciliation failed.")
        })?;
    Ok(Json(report))
}

pub async fn retry_memory_document_sync(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentRetryResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory document outbox is unavailable.")
    })?;
    let requeued = outbox
        .retry_dead_letter(workspace_id, memory_id, version, user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory outbox recovery failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory synchronization recovery failed.")
        })?;
    if !requeued {
        return Err(api_error(StatusCode::NOT_FOUND, "Recoverable dead-letter event was not found."));
    }
    Ok(Json(MemoryDocumentRetryResponse {
        memory_id,
        version,
        sync_status: "pending",
    }))
}

pub async fn get_agent_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, memory_id, version)): Path<(Uuid, Uuid, Uuid, u64)>,
    axum::extract::Query(query): axum::extract::Query<MemoryDocumentTaskQuery>,
) -> Result<Json<MemoryDocument>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let agent_matches = state.agents.read().await.get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id);
    if !agent_matches {
        return Err(api_error(StatusCode::NOT_FOUND, "Agent not found."));
    }
    if let Some(task_id) = query.task_id {
        let task_matches = state.tasks.read().await.get(&task_id)
            .is_some_and(|task| task.workspace_id == workspace_id && task.assignee_agent_id == agent_id);
        if !task_matches {
            return Err(api_error(StatusCode::NOT_FOUND, "Task not found for this agent."));
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory document metadata is unavailable.")
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "MongoDB memory documents are unavailable.")
    })?;
    let document = outbox
        .get_authorized_document(
            repository.as_ref(),
            workspace_id,
            memory_id,
            version,
            agent_id,
            query.task_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document retrieval failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document could not be retrieved.")
        })?;
    document
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Memory document not found."))
}


#[derive(Debug, Deserialize)]
pub struct MemoryDocumentSearchRequest {
    pub query: String,
    #[serde(default)]
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub dependency_ids: BTreeSet<Uuid>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default = "default_half")]
    pub context: f32,
    #[serde(default = "default_half")]
    pub outcome: f32,
    #[serde(default = "default_half")]
    pub failure: f32,
    #[serde(default = "default_half")]
    pub evidence: f32,
}

fn default_search_limit() -> usize {
    10
}
fn default_half() -> f32 {
    0.5
}

#[derive(Debug, Serialize)]
pub struct RankedMemoryDocument {
    pub memory_id: Uuid,
    pub version: u64,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub source: String,
    pub source_kind: String,
    pub source_event_sequence: u64,
    pub qkv_attention_score: f64,
    pub qoin_score: ScoreBreakdown,
    pub qoin_reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentSearchResponse {
    pub retrieval_mode: String,
    pub qkv_model_version: String,
    pub qoin_scoring_version: String,
    pub qoin_scoring_config: QoinScoringConfig,
    pub candidates_examined: usize,
    pub context_bytes_used: usize,
    pub results: Vec<RankedMemoryDocument>,
}

fn normalized_metadata_score(document: &MemoryDocument, key: &str) -> f32 {
    document
        .key_metadata
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0) as f32)
        .unwrap_or(0.5)
}

fn metadata_dependencies(document: &MemoryDocument) -> BTreeSet<Uuid> {
    document
        .key_metadata
        .get("dependency_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect()
}

fn metadata_outcome_labels(document: &MemoryDocument) -> BTreeSet<String> {
    document
        .key_metadata
        .get("outcome_labels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub async fn search_agent_memory_documents(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<MemoryDocumentSearchRequest>,
) -> Result<Json<MemoryDocumentSearchResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let agent_matches = state.agents.read().await.get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id);
    if !agent_matches {
        return Err(api_error(StatusCode::NOT_FOUND, "Agent not found."));
    }
    if input.query.trim().is_empty()
        || input.query.len() > 4096
        || input.tags.len() > 64
        || input.dependency_ids.len() > 64
    {
        return Err(api_error(StatusCode::BAD_REQUEST, "Query, tags, or dependencies exceed retrieval limits."));
    }
    for score in [input.context, input.outcome, input.failure, input.evidence] {
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err(api_error(StatusCode::BAD_REQUEST, "Qoin query features must be normalized to [0, 1]."));
        }
    }
    if let Some(task_id) = input.task_id {
        let task_matches = state.tasks.read().await.get(&task_id)
            .is_some_and(|task| task.workspace_id == workspace_id && task.assignee_agent_id == agent_id);
        if !task_matches {
            return Err(api_error(StatusCode::NOT_FOUND, "Task not found for this agent."));
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory document metadata is unavailable.")
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "MongoDB memory documents are unavailable.")
    })?;
    let refs = outbox
        .list_authorized_document_refs(workspace_id, agent_id, input.task_id, 200)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document candidate lookup failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document search failed.")
        })?;

    let fallback_provider = LexicalMemoryVectorProvider;
    let vector_provider: &dyn MemoryVectorProvider = state
        .memory_vector_provider
        .as_deref()
        .unwrap_or(&fallback_provider);
    let query_vector = vector_provider
        .key_for_text(&input.query)
        .await
        .map_err(|provider_error| match provider_error {
            MemoryVectorProviderError::EmptyQuery => {
                api_error(StatusCode::BAD_REQUEST, "The query contains no searchable terms.")
            }
            _ => api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory embedding provider is unavailable."),
        })?;
    if query_vector.model_version != vector_provider.model_version() {
        return Err(api_error(StatusCode::BAD_GATEWAY, "Memory embedding provider returned an incompatible model version."));
    }
    let qkv_model_version = query_vector.model_version.clone();
    let query_values = query_vector.values.clone();

    let mut documents = HashMap::new();
    let mut attention_candidates = Vec::new();
    let mut indexed_memories = Vec::new();
    for (memory_id, version) in refs {
        let document = outbox
            .get_authorized_document(
                repository.as_ref(),
                workspace_id,
                memory_id,
                version,
                agent_id,
                input.task_id,
            )
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "memory document authorization or integrity check failed");
                api_error(StatusCode::INTERNAL_SERVER_ERROR, "Memory document search failed.")
            })?;
        let Some(document) = document else {
            continue;
        };

        let mut key_text = document.title.clone();
        key_text.push(' ');
        key_text.push_str(&document.content);
        for tag in &document.tags {
            key_text.push(' ');
            key_text.push_str(tag);
        }
        let key = if let Some(stored_key) = document.key_metadata.get("qkv_key") {
            let stored_key: QoinVector = serde_json::from_value(stored_key.clone())
                .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Stored QKV key is invalid."))?;
            stored_key.validate()
                .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Stored QKV key failed validation."))?;
            if document.qkv_version != stored_key.model_version
                || document.key_metadata.get("qkv_key_version").and_then(Value::as_str)
                    != Some(stored_key.model_version.as_str())
            {
                return Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "Stored QKV key version does not match its document."));
            }
            stored_key
        } else {
            // Backward compatibility for documents created before persistent
            // QKV keys were stored in MongoDB.
            vector_provider
                .key_for_text(&key_text)
                .await
                .map_err(|_| api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory embedding provider is unavailable."))?
        };
        if document.qkv_version != key.model_version {
            // Never compare vectors from different model spaces.
            continue;
        }
        let key_values = key.values.clone();
        let dependencies = metadata_dependencies(&document);
        let outcome = normalized_metadata_score(&document, "outcome");
        let evidence = normalized_metadata_score(&document, "evidence");
        let source_sequence = i64::try_from(document.source_event_sequence)
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Invalid memory source sequence."))?;
        attention_candidates.push(AttentionMemory {
            memory_id,
            source_kind: document.source_kind.clone(),
            key,
            value: memory_id.to_string(),
            outcome_score: outcome,
            evidence_score: evidence,
            source_event_sequence: source_sequence,
            tags: document.tags.iter().cloned().collect(),
            dependency_ids: dependencies.clone(),
        });

        let visibility = match document.scope.as_str() {
            "agent_private" => MemoryVisibility::AgentPrivate,
            "task_shared" => MemoryVisibility::TaskShared,
            "project_shared" => MemoryVisibility::ProjectShared,
            _ => MemoryVisibility::WorkspaceShared,
        };
        indexed_memories.push(IndexedMemory {
            id: memory_id,
            workspace_id,
            chat_id: Uuid::nil(),
            task_id: document.task_id,
            project_id: document.project_id,
            owner_agent_id: document.owner_id,
            visibility,
            status: MemoryStatus::Approved,
            vector: MemoryVector {
                semantic: key_values,
                context: normalized_metadata_score(&document, "context"),
                outcome,
                failure: normalized_metadata_score(&document, "failure"),
                evidence,
            },
            tags: document.tags.iter().cloned().collect(),
            dependency_ids: dependencies,
            outcome_labels: metadata_outcome_labels(&document),
            source_event_ids: Vec::new(),
            created_at_unix: document.created_at.timestamp(),
            superseded_by: None,
        });
        documents.insert(memory_id, document);
    }

    let attention = qkv_attention(
        &AttentionQuery {
            query: query_vector,
            heads: 4,
            tags: input.tags.clone(),
            dependency_ids: input.dependency_ids.clone(),
        },
        &attention_candidates,
    )
    .map_err(|_| api_error(StatusCode::BAD_REQUEST, "QKV query could not be evaluated."))?;
    let attention_scores: HashMap<Uuid, f64> = attention
        .into_iter()
        .map(|score| (score.memory_id, score.total))
        .collect();

    let qoin = retrieve_memories_with_config(
        &MemoryQuery {
            workspace_id,
            requester_agent_id: Some(agent_id),
            task_id: input.task_id,
            project_id: None,
            semantic: query_values,
            context: input.context,
            outcome: input.outcome,
            failure: input.failure,
            evidence: input.evidence,
            now_unix: Utc::now().timestamp(),
            tags: input.tags,
            dependency_ids: input.dependency_ids,
            outcome_labels: BTreeSet::new(),
            limit: input.limit.clamp(1, 20),
        },
        &indexed_memories,
        &state.qoin_memory_scoring,
    )
    .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Qoin memory ranking could not be evaluated."))?;
    let qoin_scoring_version = qoin.scoring_config.version.clone();
    let qoin_scoring_config = qoin.scoring_config.clone();

    const MAX_CONTEXT_BYTES: usize = 64 * 1024;
    let mut context_bytes_used = 0_usize;
    let mut results = Vec::with_capacity(qoin.ranked.len());
    for ranked in qoin.ranked {
        let Some(document) = documents.remove(&ranked.memory_id) else {
            continue;
        };
        if document.content.len() > MAX_CONTEXT_BYTES.saturating_sub(context_bytes_used) {
            continue;
        }
        context_bytes_used += document.content.len();
        results.push(RankedMemoryDocument {
            memory_id: document.memory_id,
            version: document.version,
            title: document.title,
            content: document.content,
            content_hash: document.content_hash,
            source: document.source,
            source_kind: document.source_kind,
            source_event_sequence: document.source_event_sequence,
            qkv_attention_score: attention_scores.get(&ranked.memory_id).copied().unwrap_or(0.0),
            qoin_score: ranked.score,
            qoin_reasons: ranked.reasons,
        });
    }

    let retrieval_mode = if qkv_model_version == LEXICAL_MEMORY_VECTOR_VERSION {
        "deterministic_lexical_qkv_v1".to_owned()
    } else {
        "configured_embedding_provider".to_owned()
    };
    Ok(Json(MemoryDocumentSearchResponse {
        retrieval_mode,
        qkv_model_version,
        qoin_scoring_version,
        qoin_scoring_config,
        candidates_examined: indexed_memories.len(),
        context_bytes_used,
        results,
    }))
}




#[derive(Debug, Serialize)]
pub struct MemoryEpochDocumentResponse {
    pub session_id: Uuid,
    pub epoch_id: Uuid,
    pub documents: Vec<MemoryDocument>,
}

/// Returns MongoDB values from the immutable epoch already pinned to this
/// agent session. The caller cannot choose a different epoch.
pub async fn get_agent_session_documents(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<MemoryEpochDocumentResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user.user_id, workspace_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Workspace access could not be verified."))?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let agent_matches = state.agents.read().await.get(&agent_id)
        .is_some_and(|agent| agent.workspace_id == workspace_id);
    if !agent_matches {
        return Err(api_error(StatusCode::NOT_FOUND, "Agent not found."));
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "Memory epoch metadata is unavailable.")
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, "MongoDB memory documents are unavailable.")
    })?;
    let (epoch_id, task_id, documents) = outbox
        .list_session_authorized_documents(
            repository.as_ref(),
            workspace_id,
            session_id,
            agent_id,
            100,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "pinned session memory hydration failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Pinned session memory documents could not be retrieved.")
        })?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Memory session not found for this agent."))?;

    if let Some(task_id) = task_id {
        let task_matches = state.tasks.read().await.get(&task_id)
            .is_some_and(|task| task.workspace_id == workspace_id && task.assignee_agent_id == agent_id);
        if !task_matches {
            return Err(api_error(StatusCode::NOT_FOUND, "Session task is no longer assigned to this agent."));
        }
    }
    Ok(Json(MemoryEpochDocumentResponse { session_id, epoch_id, documents }))
}
