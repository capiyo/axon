pub async fn create_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<CreateMemoryDocumentRequest>,
) -> Result<(StatusCode, Json<CreateMemoryDocumentResponse>), (StatusCode, Json<ApiError>)> {
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

    if input.content.trim().is_empty()
        || input.content.len() > 256 * 1024
        || input.title.trim().is_empty()
        || input.title.len() > 300
        || input.tags.len() > 64
        || input
            .tags
            .iter()
            .any(|tag| tag.trim().is_empty() || tag.len() > 64)
        || input.document_type.trim().is_empty()
        || input.document_type.len() > 80
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Memory document fields exceed their validation limits.",
        ));
    }

    match input.scope.as_str() {
        "workspace_shared" => {
            if input.task_id.is_some()
                || input.project_id.is_some()
                || input.owner_agent_id.is_some()
            {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "Workspace-shared memory cannot carry narrower scope identifiers.",
                ));
            }
        }
        "task_shared" => {
            let Some(task_id) = input.task_id else {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "Task-shared memory requires task_id.",
                ));
            };
            let task_matches = state
                .tasks
                .read()
                .await
                .get(&task_id)
                .is_some_and(|task| task.workspace_id == workspace_id);
            if !task_matches || input.project_id.is_some() || input.owner_agent_id.is_some() {
                return Err(api_error(
                    StatusCode::NOT_FOUND,
                    "Task scope was not found in this workspace.",
                ));
            }
        }
        "agent_private" => {
            let Some(agent_id) = input.owner_agent_id else {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "Agent-private memory requires owner_agent_id.",
                ));
            };
            let agent_matches = state
                .is_agent_owner(user.user_id, workspace_id, agent_id)
                .await;
            if !agent_matches || input.task_id.is_some() || input.project_id.is_some() {
                return Err(api_error(
                    StatusCode::NOT_FOUND,
                    "Private-memory agent was not found in this workspace.",
                ));
            }
        }
        "project_shared" => {
            let Some(project_id) = input.project_id else {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "Project-shared memory requires project_id.",
                ));
            };
            if input.task_id.is_some() || input.owner_agent_id.is_some() {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "Project-shared memory cannot carry task or private-agent identifiers.",
                ));
            }
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
                    "Project was not found in this workspace.",
                ));
            }
        }
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "Unsupported memory scope.",
            ))
        }
    }

    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document persistence is unavailable.",
        )
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
            MemoryVectorProviderError::EmptyQuery => api_error(
                StatusCode::BAD_REQUEST,
                "Memory document contains no searchable terms.",
            ),
            _ => api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Memory embedding provider is unavailable.",
            ),
        })?;
    validate_memory_key(&qkv_key, vector_provider.model_version()).map_err(|_| {
        api_error(
            StatusCode::BAD_GATEWAY,
            "Memory embedding provider returned an invalid or incompatible key.",
        )
    })?;
    let mut key_metadata = match input.key_metadata {
        Value::Object(metadata) => metadata,
        Value::Null => Map::new(),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "key_metadata must be a JSON object.",
            ))
        }
    };
    let serialized_key = serde_json::to_value(&qkv_key)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "QKV key could not be serialized."))?;
    key_metadata.insert("qkv_key".into(), serialized_key);
    key_metadata.insert(
        "qkv_key_version".into(),
        Value::String(qkv_key.model_version.clone()),
    );

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
    document
        .validate()
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "Memory document is invalid."))?;
    outbox
        .enqueue(&mut document, user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document could not be queued");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory document could not be persisted.",
            )
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

