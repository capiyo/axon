impl MemoryOutbox {
    pub async fn list_authorized_document_versions(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        project_id: Option<Uuid>,
        before_version: Option<u64>,
        requested_limit: usize,
    ) -> Result<Vec<AuthorizedMemoryDocumentVersion>, MemoryOutboxError> {
        let limit = i64::try_from(requested_limit.clamp(1, 101)).map_err(|_| {
            sqlx::Error::Protocol("version history limit exceeds SQLite range".into())
        })?;
        let before_version = before_version
            .map(i64::try_from)
            .transpose()
            .map_err(|_| sqlx::Error::Protocol("version cursor exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT version, content_hash, source_event_sequence, qkv_version, qoin_vector_version, scope, project_id, owner_id, task_id \
             FROM memory_document_versions \
             WHERE workspace_id = ? AND memory_id = ? AND lifecycle_status = 'approved' \
             AND sync_status IN ('stored', 'indexed') \
             AND (scope = 'workspace_shared' OR (scope = 'agent_private' AND owner_id = ?) \
                  OR (scope = 'task_shared' AND task_id = ?) \
                  OR (scope = 'project_shared' AND project_id = ? AND EXISTS (\
                      SELECT 1 FROM project_memberships pm JOIN projects p ON p.id = pm.project_id \
                      JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id \
                      WHERE pm.project_id = memory_document_versions.project_id AND pm.user_id = ? \
                      AND pm.revoked_at IS NULL AND p.workspace_id = memory_document_versions.workspace_id))) \
             AND (? IS NULL OR version < ?) ORDER BY version DESC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(requester_agent_id.to_string())
        .bind(task_id.map(|value| value.to_string()))
        .bind(project_id.map(|value| value.to_string()))
        .bind(requester_user_id.to_string())
        .bind(before_version)
        .bind(before_version)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            let version: i64 = row.try_get("version")?;
            let source_sequence: i64 = row.try_get("source_event_sequence")?;
            let task_id_text: Option<String> = row.try_get("task_id")?;
            versions.push(AuthorizedMemoryDocumentVersion {
                version: u64::try_from(version)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                content_hash: row.try_get("content_hash")?,
                source_event_sequence: u64::try_from(source_sequence)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                qkv_version: row.try_get("qkv_version")?,
                qoin_vector_version: row.try_get("qoin_vector_version")?,
                scope: row.try_get("scope")?,
                project_id: row
                    .try_get::<Option<String>, _>("project_id")?
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                owner_id: row
                    .try_get::<Option<String>, _>("owner_id")?
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                task_id: task_id_text
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
            });
        }
        Ok(versions)
    }

    /// Read-only, bounded consistency audit for one workspace. It reports
    /// mismatches but never repairs or exposes document content.
    pub async fn reconcile_workspace(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        requested_limit: usize,
    ) -> Result<MemoryReconciliationReport, MemoryOutboxError> {
        let limit = requested_limit.clamp(1, 5_000);
        let limit_sql = i64::try_from(limit).map_err(|_| {
            sqlx::Error::Protocol("reconciliation limit exceeds SQLite range".into())
        })?;
        let rows = sqlx::query(
            "SELECT memory_id, version, scope, content_hash, source_event_sequence, qkv_version, qoin_vector_version, sync_status \
             FROM memory_document_versions WHERE workspace_id = ? ORDER BY memory_id ASC, version ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(limit_sql)
        .fetch_all(&self.pool)
        .await?;
        let documents = repository
            .list_workspace_documents(workspace_id, limit)
            .await?;
        let mut mongo_by_key = HashMap::new();
        let mut issues = Vec::new();

        for document in &documents {
            let key = (document.memory_id, document.version);
            if mongo_by_key.insert(key, document).is_some() {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "duplicate_mongo_version".into(),
                });
            }
            if document.validate().is_err() {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "invalid_mongo_document".into(),
                });
            }
        }

        let mut sqlite_keys = HashSet::with_capacity(rows.len());
        for row in &rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_sql: i64 = row.try_get("version")?;
            let version = u64::try_from(version_sql)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let key = (memory_id, version);
            sqlite_keys.insert(key);
            let scope: String = row.try_get("scope")?;
            let content_hash: String = row.try_get("content_hash")?;
            let source_sequence: i64 = row.try_get("source_event_sequence")?;
            let qkv_version: String = row.try_get("qkv_version")?;
            let qoin_version: Option<String> = row.try_get("qoin_vector_version")?;
            let sync_status: String = row.try_get("sync_status")?;

            let Some(document) = mongo_by_key.get(&key) else {
                issues.push(MemoryReconciliationIssue {
                    memory_id,
                    version,
                    kind: match sync_status.as_str() {
                        "pending" => "mongo_document_pending_delivery",
                        "failed" => "mongo_document_missing_after_failure",
                        _ => "sqlite_version_missing_in_mongo",
                    }
                    .into(),
                });
                continue;
            };

            let source_sequence = u64::try_from(source_sequence)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let mut add_issue = |kind: &str| {
                issues.push(MemoryReconciliationIssue {
                    memory_id,
                    version,
                    kind: kind.to_owned(),
                });
            };
            if document.workspace_id != workspace_id {
                add_issue("workspace_id_mismatch");
            }
            if document.scope != scope {
                add_issue("scope_mismatch");
            }
            if document.content_hash != content_hash {
                add_issue("content_hash_mismatch");
            }
            if document.source_event_sequence != source_sequence {
                add_issue("source_event_sequence_mismatch");
            }
            if document.qkv_version != qkv_version {
                add_issue("qkv_model_version_mismatch");
            }
            if document.qoin_vector_version != qoin_version {
                add_issue("qoin_vector_version_mismatch");
            }
            match document
                .key_metadata
                .get("qkv_key_version")
                .and_then(serde_json::Value::as_str)
            {
                Some(key_version) if key_version == document.qkv_version => {}
                Some(_) => add_issue("qkv_key_version_mismatch"),
                None => add_issue("persisted_qkv_key_missing"),
            }
        }

        for document in &documents {
            if !sqlite_keys.contains(&(document.memory_id, document.version)) {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "mongo_document_without_sqlite_authority".into(),
                });
            }
        }

        let status_rows = sqlx::query(
            "SELECT o.status, COUNT(*) AS count FROM memory_document_outbox o \
             JOIN memory_document_versions v ON v.memory_id = o.memory_id AND v.version = o.version \
             WHERE v.workspace_id = ? GROUP BY o.status",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut pending = 0_u64;
        let mut processing = 0_u64;
        let mut dead_letter = 0_u64;
        for row in status_rows {
            let status: String = row.try_get("status")?;
            let count: i64 = row.try_get("count")?;
            let count =
                u64::try_from(count).map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            match status.as_str() {
                "pending" => pending = count,
                "processing" => processing = count,
                "dead_letter" => dead_letter = count,
                _ => {}
            }
        }

        Ok(MemoryReconciliationReport {
            workspace_id,
            sqlite_versions_scanned: rows.len(),
            mongo_documents_scanned: documents.len(),
            pending_outbox_events: pending,
            processing_outbox_events: processing,
            dead_letter_outbox_events: dead_letter,
            scan_limit_reached: rows.len() == limit || documents.len() == limit,
            issues,
        })
    }

    /// Requeues a valid dead-letter event without changing the immutable
    /// document version. The operation and its audit event are atomic in SQLite.
    pub async fn retry_dead_letter(
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
        let payload_row = sqlx::query(
            "SELECT payload_json FROM memory_document_outbox \
             WHERE memory_id = ? AND version = ? AND status = 'dead_letter'",
        )
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(payload_row) = payload_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let payload_json: Option<String> = payload_row.try_get("payload_json")?;
        let Some(payload_json) = payload_json else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let document = match serde_json::from_str::<MemoryDocument>(&payload_json) {
            Ok(document) => document,
            Err(_) => {
                transaction.rollback().await?;
                return Ok(false);
            }
        };
        let metadata_row = sqlx::query(
            "SELECT workspace_id, content_hash, sync_status FROM memory_document_versions \
             WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(metadata_row) = metadata_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let stored_workspace: String = metadata_row.try_get("workspace_id")?;
        let stored_hash: String = metadata_row.try_get("content_hash")?;
        let sync_status: String = metadata_row.try_get("sync_status")?;
        if document.validate().is_err()
            || document.memory_id != memory_id
            || document.workspace_id != workspace_id
            || document.version != version as u64
            || stored_workspace != workspace_id.to_string()
            || stored_hash != document.content_hash
            || sync_status != "failed"
        {
            transaction.rollback().await?;
            return Ok(false);
        }

        let updated = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'pending', attempts = 0, available_at = ?, \
             lease_owner = NULL, lease_until = NULL, last_error = NULL, updated_at = ? \
             WHERE memory_id = ? AND version = ? AND status = 'dead_letter' AND payload_json IS NOT NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }

        let metadata = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'pending', updated_at = ? \
             WHERE workspace_id = ? AND memory_id = ? AND version = ? AND sync_status = 'failed'",
        )
        .bind(&now)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if metadata.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }

        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, 'memory_document.outbox_requeued', 'memory_document', ?, ?, ?)",
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

    /// Lists only approved, synchronized document versions visible to an
    /// agent/task pair. Project-scoped documents remain excluded until project
    /// membership authorization is available.
}
