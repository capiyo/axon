impl MemoryOutbox {
    pub async fn list_authorized_document_refs(
        &self,
        workspace_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        project_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<(Uuid, u64)>, MemoryOutboxError> {
        let task_id_text = task_id.map(|id| id.to_string());
        let project_id_text = project_id.map(|id| id.to_string());
        let limit = i64::try_from(limit.clamp(1, 200))
            .map_err(|_| sqlx::Error::Protocol("retrieval limit exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT v.memory_id, v.version FROM memory_document_versions v \
             WHERE v.workspace_id = ? AND v.lifecycle_status = 'approved' \
             AND v.sync_status IN ('stored', 'indexed') \
             AND (v.scope = 'workspace_shared' OR (v.scope = 'agent_private' AND v.owner_id = ?) \
             OR (v.scope = 'task_shared' AND v.task_id = ?) \
             OR (v.scope = 'project_shared' AND v.project_id = ? AND EXISTS (\
                 SELECT 1 FROM project_memberships pm JOIN projects p ON p.id = pm.project_id \
                 JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id \
                 WHERE pm.project_id = v.project_id AND pm.user_id = ? AND pm.revoked_at IS NULL \
                 AND p.workspace_id = v.workspace_id))) \
             AND NOT EXISTS (SELECT 1 FROM memory_document_versions newer \
                 WHERE newer.workspace_id = v.workspace_id AND newer.memory_id = v.memory_id \
                 AND newer.version > v.version \
                 AND (newer.lifecycle_status IN ('archived', 'deleted', 'rejected', 'superseded') \
                      OR (newer.lifecycle_status = 'approved' AND newer.sync_status IN ('stored', 'indexed')))) \
             ORDER BY v.updated_at DESC, v.memory_id ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(requester_agent_id.to_string())
        .bind(task_id_text)
        .bind(project_id_text)
        .bind(requester_user_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let memory_id: String = row.try_get("memory_id")?;
                let version: i64 = row.try_get("version")?;
                let memory_id = Uuid::parse_str(&memory_id)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
                let version = u64::try_from(version)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
                Ok((memory_id, version))
            })
            .collect()
    }

    /// Returns a MongoDB document only when SQLite confirms its workspace,
    /// approval state, synchronization state, and agent/task visibility.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_authorized_document(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
    ) -> Result<Option<MemoryDocument>, MemoryOutboxError> {
        let version_sql = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let row = sqlx::query(
            "SELECT scope, owner_id, task_id, project_id, document_type, content_hash, \
             source_event_sequence, qkv_version, qoin_vector_version, lifecycle_status, sync_status \
             FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let scope: String = row.try_get("scope")?;
        let owner_id: Option<String> = row.try_get("owner_id")?;
        let stored_task_id: Option<String> = row.try_get("task_id")?;
        let stored_project_id: Option<String> = row.try_get("project_id")?;
        let stored_document_type: String = row.try_get("document_type")?;
        let expected_hash: String = row.try_get("content_hash")?;
        let stored_source_sequence: i64 = row.try_get("source_event_sequence")?;
        let stored_qkv_version: String = row.try_get("qkv_version")?;
        let stored_qoin_version: Option<String> = row.try_get("qoin_vector_version")?;
        let lifecycle_status: String = row.try_get("lifecycle_status")?;
        let sync_status: String = row.try_get("sync_status")?;
        if lifecycle_status != "approved" || !matches!(sync_status.as_str(), "stored" | "indexed") {
            return Ok(None);
        }
        let superseded_by_lifecycle: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM memory_document_versions newer
             WHERE newer.workspace_id = ? AND newer.memory_id = ? AND newer.version > ?
               AND (newer.lifecycle_status IN ('archived', 'deleted', 'rejected', 'superseded')
                    OR (newer.lifecycle_status = 'approved' AND newer.sync_status IN ('stored', 'indexed')))
             LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&self.pool)
        .await?;
        if superseded_by_lifecycle.is_some() {
            return Ok(None);
        }

        let requester_agent_id_text = requester_agent_id.to_string();
        let requester_task_id_text = task_id.map(|id| id.to_string());
        let visible = match scope.as_str() {
            "workspace_shared" => true,
            "agent_private" => owner_id.as_deref() == Some(requester_agent_id_text.as_str()),
            "task_shared" => {
                task_id.is_some() && stored_task_id.as_deref() == requester_task_id_text.as_deref()
            }
            "project_shared" => {
                if let Some(project_id) = stored_project_id.as_deref() {
                    let membership: Option<i64> = sqlx::query_scalar(
                        "SELECT 1 FROM project_memberships pm
                         JOIN projects p ON p.id = pm.project_id
                         JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                         WHERE pm.project_id = ? AND pm.user_id = ?
                           AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                    )
                    .bind(project_id)
                    .bind(requester_user_id.to_string())
                    .bind(workspace_id.to_string())
                    .fetch_optional(&self.pool)
                    .await?;
                    membership.is_some()
                } else {
                    false
                }
            }
            _ => false,
        };
        if !visible {
            return Ok(None);
        }

        let Some(document) = repository.get_version(memory_id, version).await? else {
            return Err(MemoryOutboxError::VersionConflict);
        };
        document.validate()?;
        if document.memory_id != memory_id
            || document.version != version
            || document.workspace_id != workspace_id
            || document.content_hash != expected_hash
            || document.scope != scope
            || document.document_type != stored_document_type
            || document.source_event_sequence
                != u64::try_from(stored_source_sequence)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
            || document.qkv_version != stored_qkv_version
            || document.qoin_vector_version != stored_qoin_version
            || document.project_id.map(|id| id.to_string()) != stored_project_id
            || document.owner_id.map(|id| id.to_string()) != owner_id
            || document.task_id.map(|id| id.to_string()) != stored_task_id
        {
            return Err(MemoryOutboxError::VersionConflict);
        }
        // Reflect the authoritative SQLite approval state in the returned view;
        // MongoDB's stored status is intentionally not an authorization source.
        let mut document = document;
        document.status = "ready".into();
        Ok(Some(document))
    }

    /// Hydrates only the exact MongoDB document versions pinned by an immutable
    /// memory epoch. Current lifecycle changes do not rewrite historical epoch
    /// membership; SQLite epoch pins remain the authority for this snapshot.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_epoch_authorized_documents(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        epoch_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Option<Vec<MemoryDocument>>, MemoryOutboxError> {
        let epoch_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM memory_epochs WHERE id = ? AND workspace_id = ?")
                .bind(epoch_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        if epoch_exists.is_none() {
            return Ok(None);
        }

        let limit = i64::try_from(limit.clamp(1, 100)).map_err(|_| {
            sqlx::Error::Protocol("epoch document limit exceeds SQLite range".into())
        })?;
        let rows = sqlx::query(
            "SELECT e.memory_id, e.document_version, e.content_hash, e.scope, e.project_id, e.task_id, e.owner_id, \
                    e.source_event_sequence, e.qkv_version, e.qoin_vector_version \
             FROM memory_document_epoch_items e \
             WHERE e.epoch_id = ? AND e.workspace_id = ? \
             AND NOT EXISTS (SELECT 1 FROM memory_document_epoch_items newer \
                 WHERE newer.epoch_id = e.epoch_id AND newer.memory_id = e.memory_id \
                 AND newer.document_version > e.document_version) \
             ORDER BY e.source_event_sequence DESC, e.memory_id ASC LIMIT ?",
        )
        .bind(epoch_id.to_string())
        .bind(workspace_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut documents = Vec::with_capacity(rows.len());
        let requester_agent_id_text = requester_agent_id.to_string();
        let requester_user_id_text = requester_user_id.to_string();
        let requester_task_id_text = task_id.map(|id| id.to_string());
        for row in rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_raw: i64 = row.try_get("document_version")?;
            let version = u64::try_from(version_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let expected_hash: String = row.try_get("content_hash")?;
            let scope: String = row.try_get("scope")?;
            let project_id: Option<String> = row.try_get("project_id")?;
            let stored_task_id: Option<String> = row.try_get("task_id")?;
            let owner_id: Option<String> = row.try_get("owner_id")?;
            let source_sequence_raw: i64 = row.try_get("source_event_sequence")?;
            let source_sequence = u64::try_from(source_sequence_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let qkv_version: String = row.try_get("qkv_version")?;
            let qoin_vector_version: Option<String> = row.try_get("qoin_vector_version")?;

            let visible = match scope.as_str() {
                "workspace_shared" => true,
                "agent_private" => owner_id.as_deref() == Some(requester_agent_id_text.as_str()),
                "task_shared" => {
                    task_id.is_some()
                        && stored_task_id.as_deref() == requester_task_id_text.as_deref()
                }
                "project_shared" => {
                    if let Some(project_id) = project_id.as_deref() {
                        let membership: Option<i64> = sqlx::query_scalar(
                            "SELECT 1 FROM project_memberships pm
                             JOIN projects p ON p.id = pm.project_id
                             JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                             WHERE pm.project_id = ? AND pm.user_id = ?
                               AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                        )
                        .bind(project_id)
                        .bind(&requester_user_id_text)
                        .bind(workspace_id.to_string())
                        .fetch_optional(&self.pool)
                        .await?;
                        membership.is_some()
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if !visible {
                continue;
            }

            // Confirm the authoritative version record still agrees with the
            // immutable epoch before hydrating its MongoDB value.
            let metadata = sqlx::query(
                "SELECT content_hash, sync_status FROM memory_document_versions \
                 WHERE workspace_id = ? AND memory_id = ? AND version = ?",
            )
            .bind(workspace_id.to_string())
            .bind(memory_id.to_string())
            .bind(version_raw)
            .fetch_optional(&self.pool)
            .await?;
            let Some(metadata) = metadata else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            let current_hash: String = metadata.try_get("content_hash")?;
            let sync_status: String = metadata.try_get("sync_status")?;
            if current_hash != expected_hash
                || !matches!(sync_status.as_str(), "stored" | "indexed")
            {
                return Err(MemoryOutboxError::VersionConflict);
            }

            let Some(mut document) = repository.get_version(memory_id, version).await? else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            document.validate()?;
            if document.memory_id != memory_id
                || document.version != version
                || document.workspace_id != workspace_id
                || document.content_hash != expected_hash
                || document.scope != scope
                || document.project_id.map(|id| id.to_string()) != project_id
                || document.task_id.map(|id| id.to_string()) != stored_task_id
                || document.owner_id.map(|id| id.to_string()) != owner_id
                || document.source_event_sequence != source_sequence
                || document.qkv_version != qkv_version
                || document.qoin_vector_version != qoin_vector_version
            {
                return Err(MemoryOutboxError::VersionConflict);
            }
            document.status = "ready".into();
            documents.push(document);
        }
        Ok(Some(documents))
    }

    /// Resolves the immutable epoch from the agent's persisted session before
    /// hydrating MongoDB values. Callers cannot substitute an arbitrary epoch.
    pub async fn list_session_authorized_documents(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        session_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        limit: usize,
    ) -> Result<Option<(Uuid, Option<Uuid>, Option<String>, Vec<MemoryDocument>)>, MemoryOutboxError>
    {
        let session = sqlx::query(
            "SELECT s.memory_epoch_id, s.task_id, \
             (SELECT d.qoin_scoring_version FROM memory_document_epoch_items d \
              WHERE d.epoch_id = s.memory_epoch_id ORDER BY d.memory_id LIMIT 1) AS qoin_scoring_version \
             FROM agent_memory_sessions s \
             WHERE s.session_id = ? AND s.workspace_id = ? AND s.agent_id = ?",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(requester_agent_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(session) = session else {
            return Ok(None);
        };
        let epoch_id_text: String = session.try_get("memory_epoch_id")?;
        let epoch_id = Uuid::parse_str(&epoch_id_text)
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let qoin_scoring_version: Option<String> = session.try_get("qoin_scoring_version")?;
        let distinct_policy_versions: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT qoin_scoring_version) FROM memory_document_epoch_items WHERE epoch_id = ?",
        )
        .bind(epoch_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if distinct_policy_versions > 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let task_id_text: Option<String> = session.try_get("task_id")?;
        let task_id = task_id_text
            .map(|value| Uuid::parse_str(&value))
            .transpose()
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let Some(documents) = self
            .list_epoch_authorized_documents(
                repository,
                workspace_id,
                epoch_id,
                requester_agent_id,
                requester_user_id,
                task_id,
                limit,
            )
            .await?
        else {
            return Err(MemoryOutboxError::VersionConflict);
        };
        Ok(Some((epoch_id, task_id, qoin_scoring_version, documents)))
    }

    pub async fn mark_indexed(
        &self,
        memory_id: Uuid,
        version: u64,
        expected_hash: &str,
    ) -> Result<(), MemoryOutboxError> {
        let version = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let result = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'indexed', updated_at = ?              WHERE memory_id = ? AND version = ? AND content_hash = ? AND sync_status = 'stored'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(memory_id.to_string())
        .bind(version)
        .bind(expected_hash)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        Ok(())
    }

    /// Polls durable work until the supplied shutdown channel is closed or set.
}
