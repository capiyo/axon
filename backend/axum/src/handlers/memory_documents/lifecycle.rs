pub async fn approve_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentApprovalResponse>, (StatusCode, Json<ApiError>)> {
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
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document storage is unavailable.",
        )
    })?;
    let approved = outbox
        .approve(workspace_id, memory_id, version, user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory document approval failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory document approval failed.",
            )
        })?;
    if !approved {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "Proposed memory document was not found.",
        ));
    }
    Ok(Json(MemoryDocumentApprovalResponse {
        memory_id,
        version,
        lifecycle_status: "approved",
    }))
}

#[derive(Debug, Serialize)]
pub struct MemoryDocumentLifecycleResponse {
    pub memory_id: Uuid,
    pub previous_version: u64,
    pub current_version: u64,
    pub lifecycle_status: &'static str,
}

async fn transition_memory_document(
    state: AppState,
    user_id: Uuid,
    workspace_id: Uuid,
    memory_id: Uuid,
    version: u64,
    action: MemoryLifecycleAction,
) -> Result<Json<MemoryDocumentLifecycleResponse>, (StatusCode, Json<ApiError>)> {
    let member = state
        .is_workspace_member(user_id, workspace_id)
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
    let transitioned = outbox
        .transition_lifecycle(
            repository.as_ref(),
            workspace_id,
            memory_id,
            version,
            user_id,
            action,
        )
        .await
        .map_err(|failure| {
            tracing::error!(error = %failure, "memory lifecycle transition failed");
            match failure {
                crate::memory_outbox::MemoryOutboxError::VersionConflict => api_error(
                    StatusCode::CONFLICT,
                    "Memory version changed or failed integrity validation.",
                ),
                _ => api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Memory lifecycle transition failed.",
                ),
            }
        })?;
    if !transitioned {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "Authorized current memory version was not found.",
        ));
    }
    let lifecycle_status = match action {
        MemoryLifecycleAction::Archive => "archived",
        MemoryLifecycleAction::Restore => "approved",
        MemoryLifecycleAction::Delete => "deleted",
    };
    Ok(Json(MemoryDocumentLifecycleResponse {
        memory_id,
        previous_version: version,
        current_version: version.saturating_add(1),
        lifecycle_status,
    }))
}

pub async fn archive_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentLifecycleResponse>, (StatusCode, Json<ApiError>)> {
    transition_memory_document(
        state,
        user.user_id,
        workspace_id,
        memory_id,
        version,
        MemoryLifecycleAction::Archive,
    )
    .await
}

pub async fn restore_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentLifecycleResponse>, (StatusCode, Json<ApiError>)> {
    transition_memory_document(
        state,
        user.user_id,
        workspace_id,
        memory_id,
        version,
        MemoryLifecycleAction::Restore,
    )
    .await
}

pub async fn delete_memory_document(
    State(state): State<AppState>,
    Extension(user): Extension<Arc<AuthenticatedUser>>,
    Path((workspace_id, memory_id, version)): Path<(Uuid, Uuid, u64)>,
) -> Result<Json<MemoryDocumentLifecycleResponse>, (StatusCode, Json<ApiError>)> {
    transition_memory_document(
        state,
        user.user_id,
        workspace_id,
        memory_id,
        version,
        MemoryLifecycleAction::Delete,
    )
    .await
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
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Workspace access could not be verified.",
            )
        })?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory reconciliation is unavailable.",
        )
    })?;
    let repository = state.memory_documents.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MongoDB memory documents are unavailable.",
        )
    })?;
    let report = outbox
        .reconcile_workspace(repository.as_ref(), workspace_id, query.limit)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory reconciliation failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory reconciliation failed.",
            )
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
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Workspace access could not be verified.",
            )
        })?;
    if !member {
        return Err(api_error(StatusCode::NOT_FOUND, "Workspace not found."));
    }
    let outbox = state.memory_outbox.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Memory document outbox is unavailable.",
        )
    })?;
    let requeued = outbox
        .retry_dead_letter(workspace_id, memory_id, version, user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "memory outbox recovery failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Memory synchronization recovery failed.",
            )
        })?;
    if !requeued {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "Recoverable dead-letter event was not found.",
        ));
    }
    Ok(Json(MemoryDocumentRetryResponse {
        memory_id,
        version,
        sync_status: "pending",
    }))
}

#[derive(Debug, Deserialize)]
