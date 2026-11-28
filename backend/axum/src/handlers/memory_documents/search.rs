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
    pub results_truncated: bool,
    pub results: Vec<RankedMemoryDocument>,
}

fn bounded_ranked_memory_documents(
    candidates: Vec<RankedMemoryDocument>,
    budget_bytes: usize,
) -> Result<(Vec<RankedMemoryDocument>, usize, bool), serde_json::Error> {
    let mut used = 0usize;
    let mut selected = Vec::new();
    let mut truncated = false;
    for candidate in candidates {
        let size = serde_json::to_vec(&candidate)?.len();
        if size > budget_bytes.saturating_sub(used) {
            truncated = true;
            continue;
        }
        used += size;
        selected.push(candidate);
    }
    Ok((selected, used, truncated))
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
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Workspace access could not be verified.",
            )
        })?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    require_agent_owner(&state, user.user_id, workspace_id, agent_id).await?;
    if input.query.trim().is_empty()
        || input.query.len() > 4096
        || input.tags.len() > 64
        || input.dependency_ids.len() > 64
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Query, tags, or dependencies exceed retrieval limits.",
        ));
    }
    for score in [input.context, input.outcome, input.failure, input.evidence] {
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "Qoin query features must be normalized to [0, 1].",
            ));
        }
    }
    if let Some(task_id) = input.task_id {
        let task_matches = state.tasks.read().await.get(&task_id).is_some_and(|task| {
            task.workspace_id == workspace_id && task.assignee_agent_id == agent_id
        });
        if !task_matches || input.project_id.is_some() {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Task not found for this agent.",
            ));
        }
    }
    if let Some(project_id) = input.project_id {
        let project_member = state
            .is_project_member(workspace_id, project_id, user.user_id)
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Project membership could not be checked.",
                )
            })?;
        if !project_member {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "Project not found in this workspace.",
            ));
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document metadata is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let refs = outbox
        .list_authorized_document_refs(
            workspace_id,
            agent_id,
            user.user_id,
            input.task_id,
            input.project_id,
            200,
        )
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document candidate lookup failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory document search failed.",
            )
        })?;
    let feedback_signals = outbox
        .feedback_signals(workspace_id, &refs)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory retrieval feedback lookup failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory feedback could not be loaded.",
            )
        })?;

    if input.query.trim().is_empty() || input.query.len() > 4096 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "query must contain 1–4096 bytes.",
        ));
    }
    let fallback_provider = LexicalMemoryVectorProvider;
    let vector_provider: &dyn MemoryVectorProvider = state
        .memory_vector_provider
        .as_deref()
        .unwrap_or(&fallback_provider);
    let query_vector =
        vector_provider
            .key_for_text(&input.query)
            .await
            .map_err(|provider_error| match provider_error {
                MemoryVectorProviderError::EmptyQuery => api_error(
                    StatusCode::BAD_REQUEST,
                    "The query contains no searchable terms.",
                ),
                _ => api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Memory embedding provider is unavailable.",
                ),
            })?;
    validate_memory_key(&query_vector, vector_provider.model_version()).map_err(|_| {
        api_error(
            StatusCode::BAD_GATEWAY,
            "Memory embedding provider returned an invalid or incompatible query key.",
        )
    })?;
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
                user.user_id,
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
            let stored_key: QoinVector =
                serde_json::from_value(stored_key.clone()).map_err(|_| {
                    api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Stored QKV key is invalid.",
                    )
                })?;
            stored_key.validate().map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Stored QKV key failed validation.",
                )
            })?;
            if document.qkv_version != stored_key.model_version
                || document
                    .key_metadata
                    .get("qkv_key_version")
                    .and_then(Value::as_str)
                    != Some(stored_key.model_version.as_str())
            {
                return Err(api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Stored QKV key version does not match its document.",
                ));
            }
            stored_key
        } else {
            // Backward compatibility for documents created before persistent
            // QKV keys were stored in MongoDB.
            let key = vector_provider
                .key_for_text(&key_text)
                .await
                .map_err(|provider_error| match provider_error {
                    MemoryVectorProviderError::EmptyQuery => api_error(
                        StatusCode::BAD_REQUEST,
                        "A memory document has no searchable terms.",
                    ),
                    _ => api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Memory embedding provider is unavailable.",
                    ),
                })?;
            validate_memory_key(&key, vector_provider.model_version()).map_err(|_| {
                api_error(
                    StatusCode::BAD_GATEWAY,
                    "Memory embedding provider returned an invalid or incompatible key.",
                )
            })?;
            key
        };
        if document.qkv_version != key.model_version {
            // Never compare vectors from different model spaces.
            continue;
        }
        let key_values = key.values.clone();
        let dependencies = metadata_dependencies(&document);
        let mut outcome = normalized_metadata_score(&document, "outcome");
        let mut failure = normalized_metadata_score(&document, "failure");
        let evidence = normalized_metadata_score(&document, "evidence");
        if let Some((positive_signal, negative_signal)) =
            feedback_signals.get(&(memory_id, version))
        {
            let feedback_weight = state.qoin_memory_scoring.feedback_weight as f32;
            outcome = outcome * (1.0 - feedback_weight) + *positive_signal * feedback_weight;
            failure = failure * (1.0 - feedback_weight) + *negative_signal * feedback_weight;
        }
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
                failure,
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
            project_id: input.project_id,
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
    .map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "Qoin memory ranking could not be evaluated.",
        )
    })?;
    let qoin_scoring_version = qoin.scoring_config.effective_version();
    let qoin_scoring_config = qoin.scoring_config.clone();

    const MAX_CONTEXT_BYTES: usize = 64 * 1024;
    let mut candidates = Vec::with_capacity(qoin.ranked.len());
    for ranked in qoin.ranked {
        let Some(document) = documents.remove(&ranked.memory_id) else {
            continue;
        };
        candidates.push(RankedMemoryDocument {
            memory_id: document.memory_id,
            version: document.version,
            title: document.title,
            content: document.content,
            content_hash: document.content_hash,
            source: document.source,
            source_kind: document.source_kind,
            source_event_sequence: document.source_event_sequence,
            qkv_attention_score: attention_scores
                .get(&ranked.memory_id)
                .copied()
                .unwrap_or(0.0),
            qoin_score: ranked.score,
            qoin_reasons: ranked.reasons,
        });
    }
    let (results, context_bytes_used, results_truncated) =
        bounded_ranked_memory_documents(candidates, MAX_CONTEXT_BYTES).map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory search results could not be serialized.",
            )
        })?;

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
        results_truncated,
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
