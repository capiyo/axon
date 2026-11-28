pub struct AgentSessionContextRequest {
    #[serde(flatten)]
    pub progressive: ProgressiveRetrievalRequest,
    #[serde(default)]
    pub query_text: Option<String>,
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
#[derive(Debug, Deserialize)]
pub struct AgentSessionFeedbackRequest {
    pub memory_id: Uuid,
    pub document_version: u64,
    pub event_type: String,
    #[serde(default = "empty_feedback_evidence")]
    pub evidence: serde_json::Value,
}

fn empty_feedback_evidence() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Serialize)]
pub struct AgentSessionFeedbackResponse {
    pub feedback_id: Uuid,
    pub memory_id: Uuid,
    pub document_version: u64,
    pub event_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn record_session_feedback(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    agent_principal: Option<Extension<Arc<AuthenticatedAgent>>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<AgentSessionFeedbackRequest>,
) -> ApiResult<Json<AgentSessionFeedbackResponse>> {
    if agent_principal.is_some() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Retrieval quality feedback must be submitted by the authenticated human owner.",
        ));
    }
    require_member(&state, user.user_id, workspace_id).await?;
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory feedback storage is unavailable.",
        )
    })?;
    let feedback = outbox
        .record_retrieval_feedback(
            workspace_id,
            session_id,
            agent_id,
            user.user_id,
            input.memory_id,
            input.document_version,
            &input.event_type,
            &input.evidence,
        )
        .await
        .map_err(|failure| {
            tracing::error!(error = %failure, "memory retrieval feedback failed");
            match failure {
                crate::memory_outbox::MemoryOutboxError::VersionConflict => error(
                    StatusCode::BAD_REQUEST,
                    "Feedback event, version, or evidence is invalid.",
                ),
                _ => error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Memory feedback could not be recorded.",
                ),
            }
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Session or pinned memory version not found for this agent.",
            )
        })?;
    Ok(Json(AgentSessionFeedbackResponse {
        feedback_id: feedback.feedback_id,
        memory_id: feedback.memory_id,
        document_version: feedback.document_version,
        event_type: feedback.event_type,
        created_at: feedback.created_at,
    }))
}

pub async fn retrieve_session_context(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, agent_id, session_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<AgentSessionContextRequest>,
) -> ApiResult<Json<AgentSessionMemoryContext>> {
    require_member(&state, user.user_id, workspace_id).await?;
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document metadata is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let (epoch_id, task_id, epoch_qoin_scoring_version, pinned_documents) = outbox
        .list_session_authorized_documents(
            repository.as_ref(),
            workspace_id,
            session_id,
            agent_id,
            user.user_id,
            100,
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Pinned session memory documents could not be verified.",
            )
        })?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "Memory session not found for this agent.",
            )
        })?;

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

    let feedback_versions: Vec<(Uuid, u64)> = pinned_documents
        .iter()
        .map(|document| (document.memory_id, document.version))
        .collect();
    let feedback_signals = outbox
        .feedback_signals(workspace_id, &feedback_versions)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Pinned session feedback could not be loaded.",
            )
        })?;

    let mut feedback_weight = 0.0f32;
    if !pinned_documents.is_empty() {
        let epoch_qoin_scoring_version = epoch_qoin_scoring_version.ok_or_else(|| {
            error(
                StatusCode::CONFLICT,
                "The pinned document epoch has no Qoin scoring policy metadata.",
            )
        })?;
        let expected_qoin_scoring_version = state.qoin_memory_scoring.effective_version();
        let current_policy = epoch_qoin_scoring_version == expected_qoin_scoring_version;
        let legacy_default_policy = state.qoin_memory_scoring == QoinScoringConfig::default()
            && epoch_qoin_scoring_version
                == crate::memory_intelligence::LEGACY_QOIN_MEMORY_SCORING_VERSION;
        if !current_policy && !legacy_default_policy {
            return Err(error(
                StatusCode::CONFLICT,
                "The session epoch was created with a different Qoin scoring policy. Resume with a matching policy or create a new session.",
            ));
        }
        if current_policy {
            feedback_weight = state.qoin_memory_scoring.feedback_weight as f32;
        }
    }

    let fallback_provider = LexicalMemoryVectorProvider;
    let vector_provider: &dyn MemoryVectorProvider = state
        .memory_vector_provider
        .as_deref()
        .unwrap_or(&fallback_provider);
    let document_query_vector = if let Some(query_text) = input.query_text.as_deref() {
        if query_text.trim().is_empty() || query_text.len() > 4096 {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "query_text must contain 1–4096 bytes.",
            ));
        }
        let key = vector_provider
            .key_for_text(query_text)
            .await
            .map_err(|provider_error| match provider_error {
                MemoryVectorProviderError::EmptyQuery => error(
                    StatusCode::BAD_REQUEST,
                    "query_text contains no searchable terms.",
                ),
                _ => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Memory embedding provider is unavailable.",
                ),
            })?;
        validate_memory_key(&key, vector_provider.model_version()).map_err(|_| {
            error(
                StatusCode::BAD_GATEWAY,
                "Memory embedding provider returned an invalid or incompatible query key.",
            )
        })?;
        key
    } else if input.progressive.query.model_version == vector_provider.model_version() {
        validate_memory_key(&input.progressive.query, vector_provider.model_version()).map_err(
            |_| {
                error(
                    StatusCode::BAD_REQUEST,
                    "The supplied session query key is invalid.",
                )
            },
        )?;
        input.progressive.query.clone()
    } else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "query_text is required when the progressive SQLite vector uses a different model version from MongoDB memory keys.",
        ));
    };

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
        .map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Could not retrieve additional context from this agent's pinned memory epoch.",
            )
        })?;

    let query_vector = document_query_vector;
    let mut attention_candidates = Vec::with_capacity(pinned_documents.len());
    let mut indexed_memories = Vec::with_capacity(pinned_documents.len());
    let mut documents_by_id = HashMap::with_capacity(pinned_documents.len());
    for document in pinned_documents {
        let key = match document.key_metadata.get("qkv_key") {
            Some(stored_key) => {
                serde_json::from_value::<QoinVector>(stored_key.clone()).map_err(|_| {
                    error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "A pinned memory QKV key is invalid.",
                    )
                })?
            }
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
                        _ => error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "Memory embedding provider is unavailable.",
                        ),
                    })?
            }
        };
        validate_memory_key(&key, vector_provider.model_version()).map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "A pinned memory QKV key has an incompatible model version or vector shape.",
            )
        })?;
        if document.qkv_version != key.model_version
            || key.model_version != query_vector.model_version
        {
            return Err(error(
                StatusCode::CONFLICT,
                "The pinned memory epoch uses a different QKV model version from the configured provider.",
            ));
        }

        let mut outcome = session_memory_metadata_score(&document, "outcome");
        let evidence = session_memory_metadata_score(&document, "evidence");
        let context = session_memory_metadata_score(&document, "context");
        let mut failure = session_memory_metadata_score(&document, "failure");
        if let Some((positive_signal, negative_signal)) =
            feedback_signals.get(&(document.memory_id, document.version))
        {
            outcome = outcome * (1.0 - feedback_weight) + *positive_signal * feedback_weight;
            failure = failure * (1.0 - feedback_weight) + *negative_signal * feedback_weight;
        }
        let dependencies = session_memory_dependencies(&document);
        let visibility = session_memory_visibility(&document.scope).ok_or_else(|| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "A pinned memory document has an unsupported scope.",
            )
        })?;
        let source_sequence = i64::try_from(document.source_event_sequence).map_err(|_| {
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "A pinned memory source sequence is invalid.",
            )
        })?;
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
    .map_err(|_| {
        error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Pinned session QKV retrieval could not be evaluated.",
        )
    })?;
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
    .map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "Qoin session memory ranking could not be evaluated.",
        )
    })?;
    let qoin_scoring_version = qoin.scoring_config.effective_version();
    let qoin_scoring_config = qoin.scoring_config.clone();
    let document_candidates_examined = qoin.candidates_examined;
    let document_candidates_ranked = qoin.ranked.len();
    let mut ranked_documents = Vec::with_capacity(qoin.ranked.len());
    for ranked in qoin.ranked {
        if let Some(document) = documents_by_id.remove(&ranked.memory_id) {
            ranked_documents.push(RankedSessionMemoryDocument {
                document,
                qkv_attention_score: attention_scores
                    .get(&ranked.memory_id)
                    .copied()
                    .unwrap_or(0.0),
                qoin_score: ranked.score,
                qoin_reasons: ranked.reasons,
            });
        }
    }

    const DOCUMENT_CONTEXT_BUDGET_BYTES: usize = 256 * 1024;
    let (documents, document_context_bytes_used, documents_truncated) =
        bounded_session_context(ranked_documents, DOCUMENT_CONTEXT_BUDGET_BYTES).map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Ranked session memory context could not be serialized safely.",
            )
        })?;

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

