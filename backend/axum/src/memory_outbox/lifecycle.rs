impl MemoryOutbox {
    pub async fn approve(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        actor_user_id: Uuid,
    ) -> Result<bool, MemoryOutboxError> {
        let version = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let now = Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        let scope_row = sqlx::query(
            "SELECT scope, project_id FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ? AND version = ? AND lifecycle_status = 'proposed'",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(scope_row) = scope_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let scope: String = scope_row.try_get("scope")?;
        let project_id: Option<String> = scope_row.try_get("project_id")?;
        if scope == "project_shared" {
            let Some(project_id) = project_id else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            let authorized: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM project_memberships pm
                 JOIN projects p ON p.id = pm.project_id
                 JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                 WHERE pm.project_id = ? AND pm.user_id = ?
                   AND pm.revoked_at IS NULL AND p.workspace_id = ?",
            )
            .bind(project_id)
            .bind(actor_user_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            if authorized.is_none() {
                transaction.rollback().await?;
                return Ok(false);
            }
        }
        let changed = sqlx::query(
            "UPDATE memory_document_versions SET lifecycle_status = 'approved', updated_at = ? \
             WHERE workspace_id = ? AND memory_id = ? AND version = ? AND lifecycle_status = 'proposed'",
        )
        .bind(&now)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }

        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, 'memory_document.approved', 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(memory_id.to_string())
        .bind(&now)
        .bind(serde_json::json!({"memory_id": memory_id, "version": version}).to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Creates a new immutable lifecycle version. The original MongoDB
    /// version and every epoch pin remain untouched; SQLite continues to
    /// decide which version is eligible for current retrieval.
    pub async fn transition_lifecycle(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        actor_user_id: Uuid,
        action: MemoryLifecycleAction,
    ) -> Result<bool, MemoryOutboxError> {
        let version_sql = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT scope, project_id, owner_id, content_hash, document_type,
                    source_event_sequence, qkv_version, qoin_vector_version,
                    lifecycle_status, sync_status
             FROM memory_document_versions
             WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let scope: String = row.try_get("scope")?;
        let project_id: Option<String> = row.try_get("project_id")?;
        let owner_id: Option<String> = row.try_get("owner_id")?;
        let content_hash: String = row.try_get("content_hash")?;
        let document_type: String = row.try_get("document_type")?;
        let source_sequence: i64 = row.try_get("source_event_sequence")?;
        let qkv_version: String = row.try_get("qkv_version")?;
        let qoin_vector_version: Option<String> = row.try_get("qoin_vector_version")?;
        let lifecycle_status: String = row.try_get("lifecycle_status")?;
        let sync_status: String = row.try_get("sync_status")?;

        let latest_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if latest_version != version_sql || !matches!(sync_status.as_str(), "stored" | "indexed") {
            transaction.rollback().await?;
            return Ok(false);
        }
        let allowed_transition = match action {
            MemoryLifecycleAction::Archive => lifecycle_status == "approved",
            MemoryLifecycleAction::Delete => matches!(
                lifecycle_status.as_str(),
                "proposed" | "approved" | "archived"
            ),
            MemoryLifecycleAction::Restore => lifecycle_status == "archived",
        };
        if !allowed_transition {
            transaction.rollback().await?;
            return Ok(false);
        }

        let workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if workspace_member.is_none() {
            transaction.rollback().await?;
            return Ok(false);
        }

        match scope.as_str() {
            "project_shared" => {
                let Some(project_id) = project_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let authorized: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM project_memberships pm
                     JOIN projects p ON p.id = pm.project_id
                     JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                     WHERE pm.project_id = ? AND pm.user_id = ?
                       AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                )
                .bind(project_id)
                .bind(actor_user_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
                if authorized.is_none() {
                    transaction.rollback().await?;
                    return Ok(false);
                }
            }
            "agent_private" => {
                let Some(owner_id) = owner_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let agent_payload: Option<String> = sqlx::query_scalar(
                    "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
                )
                .bind(owner_id)
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
                let Some(agent_payload) = agent_payload else {
                    transaction.rollback().await?;
                    return Ok(false);
                };
                let agent = serde_json::from_str::<crate::storage::StoredEntity>(&agent_payload)?;
                let owned = matches!(
                    agent,
                    crate::storage::StoredEntity::Agent(ref value)
                        if value.created_by_user_id == Some(actor_user_id)
                );
                if !owned {
                    transaction.rollback().await?;
                    return Ok(false);
                }
            }
            "task_shared" | "workspace_shared" => {}
            _ => {
                transaction.rollback().await?;
                return Ok(false);
            }
        }
        transaction.commit().await?;

        let mut document = repository
            .get_version(memory_id, version)
            .await?
            .ok_or(MemoryOutboxError::VersionConflict)?;
        document.validate()?;
        if document.workspace_id != workspace_id
            || document.memory_id != memory_id
            || document.version != version
            || document.content_hash != content_hash
            || document.scope != scope
            || document.project_id.map(|id| id.to_string()) != project_id
            || document.owner_id.map(|id| id.to_string()) != owner_id
            || document.document_type != document_type
            || document.source_event_sequence
                != u64::try_from(source_sequence)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
            || document.qkv_version != qkv_version
            || document.qoin_vector_version != qoin_vector_version
        {
            return Err(MemoryOutboxError::VersionConflict);
        }

        document.version = version
            .checked_add(1)
            .ok_or_else(|| sqlx::Error::Protocol("memory version overflow".into()))?;
        document.updated_at = Utc::now();
        document.source_event_sequence = 0;
        document.status = match action {
            MemoryLifecycleAction::Archive => "archived",
            MemoryLifecycleAction::Restore => "ready",
            MemoryLifecycleAction::Delete => "deleted",
        }
        .into();
        document.content_hash = document.calculate_content_hash();
        self.enqueue(&mut document, actor_user_id).await?;
        Ok(true)
    }

    /// Records feedback only for the exact memory version pinned to the
    /// caller's authenticated agent session. Feedback never mutates a memory
    /// version or an already-published epoch.
    #[allow(clippy::too_many_arguments)]
}
