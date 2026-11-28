impl MemoryOutbox {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Atomically records an audit event, authoritative metadata, and its
    /// MongoDB write event. The audit sequence becomes the immutable source
    /// sequence stored in both SQLite metadata and the MongoDB document.
    pub async fn enqueue(
        &self,
        document: &mut MemoryDocument,
        actor_user_id: Uuid,
    ) -> Result<(), MemoryOutboxError> {
        document
            .validate()
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let version = i64::try_from(document.version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let lifecycle_status = match document.status.as_str() {
            "proposed" => "proposed",
            "archived" => "archived",
            "deleted" => "deleted",
            "ready" => "approved",
            _ => return Err(MemoryOutboxError::VersionConflict),
        };

        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query(
            "SELECT workspace_id, project_id, owner_id, task_id, scope, document_type, \
             content_hash, source_event_sequence, qkv_version, qoin_vector_version, lifecycle_status, sync_status \
             FROM memory_document_versions WHERE memory_id = ? AND version = ?",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(existing) = existing {
            let stored_workspace: String = existing.try_get("workspace_id")?;
            let stored_project: Option<String> = existing.try_get("project_id")?;
            let stored_owner: Option<String> = existing.try_get("owner_id")?;
            let stored_task: Option<String> = existing.try_get("task_id")?;
            let stored_scope: String = existing.try_get("scope")?;
            let stored_type: String = existing.try_get("document_type")?;
            let stored_hash: String = existing.try_get("content_hash")?;
            let stored_sequence: i64 = existing.try_get("source_event_sequence")?;
            let stored_qkv_version: String = existing.try_get("qkv_version")?;
            let stored_qoin_version: Option<String> = existing.try_get("qoin_vector_version")?;
            let stored_lifecycle: String = existing.try_get("lifecycle_status")?;
            let sync_status: String = existing.try_get("sync_status")?;
            if stored_workspace != document.workspace_id.to_string()
                || stored_project != document.project_id.map(|id| id.to_string())
                || stored_owner != document.owner_id.map(|id| id.to_string())
                || stored_task != document.task_id.map(|id| id.to_string())
                || stored_scope != document.scope
                || stored_type != document.document_type
                || stored_hash != document.content_hash
                || stored_sequence
                    != i64::try_from(document.source_event_sequence).map_err(|_| {
                        sqlx::Error::Protocol("source sequence exceeds SQLite range".into())
                    })?
                || stored_qkv_version != document.qkv_version
                || stored_qoin_version != document.qoin_vector_version
                || stored_lifecycle != lifecycle_status
            {
                return Err(MemoryOutboxError::VersionConflict);
            }

            if sync_status == "pending" || sync_status == "failed" {
                let payload = serde_json::to_string(document)?;
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "INSERT INTO memory_document_outbox \
                     (event_id, memory_id, version, event_type, payload_json, status, attempts, \
                      available_at, created_at, updated_at) \
                     VALUES (?, ?, ?, 'memory_document.put', ?, 'pending', 0, ?, ?, ?) \
                     ON CONFLICT(memory_id, version, event_type) DO NOTHING",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(document.memory_id.to_string())
                .bind(version)
                .bind(payload)
                .bind(&now)
                .bind(&now)
                .bind(&now)
                .execute(&mut *transaction)
                .await?;
            }
            transaction.commit().await?;
            return Ok(());
        }

        if document.source_event_sequence != 0 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let now = Utc::now().to_rfc3339();
        let audit_event_type = match lifecycle_status {
            "proposed" => "memory_document.created",
            "approved" => "memory_document.approved",
            "archived" => "memory_document.archived",
            "deleted" => "memory_document.deleted",
            _ => "memory_document.versioned",
        };
        let audit_result = sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, ?, 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(document.workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(audit_event_type)
        .bind(document.memory_id.to_string())
        .bind(&now)
        .bind(serde_json::json!({
            "memory_id": document.memory_id,
            "version": document.version,
            "content_hash": document.content_hash,
            "lifecycle_status": lifecycle_status
        }).to_string())
        .execute(&mut *transaction)
        .await?;
        let source_sequence = u64::try_from(audit_result.last_insert_rowid())
            .map_err(|_| sqlx::Error::Protocol("invalid audit source sequence".into()))?;
        document.source_event_sequence = source_sequence;
        let source_sequence_sql = i64::try_from(source_sequence)
            .map_err(|_| sqlx::Error::Protocol("source sequence exceeds SQLite range".into()))?;
        let payload = serde_json::to_string(document)?;

        sqlx::query(
            "INSERT INTO memory_document_versions \
             (memory_id, version, workspace_id, project_id, owner_id, task_id, scope, \
              document_type, content_hash, source_event_sequence, qkv_version, \
              qoin_vector_version, lifecycle_status, sync_status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .bind(document.workspace_id.to_string())
        .bind(document.project_id.map(|id| id.to_string()))
        .bind(document.owner_id.map(|id| id.to_string()))
        .bind(document.task_id.map(|id| id.to_string()))
        .bind(&document.scope)
        .bind(&document.document_type)
        .bind(&document.content_hash)
        .bind(source_sequence_sql)
        .bind(&document.qkv_version)
        .bind(&document.qoin_vector_version)
        .bind(lifecycle_status)
        .bind(document.created_at.to_rfc3339())
        .bind(document.updated_at.to_rfc3339())
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO memory_document_outbox \
             (event_id, memory_id, version, event_type, payload_json, status, attempts, \
              available_at, created_at, updated_at) \
             VALUES (?, ?, ?, 'memory_document.put', ?, 'pending', 0, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(document.memory_id.to_string())
        .bind(version)
        .bind(payload)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    async fn claim_next(
        &self,
        worker_id: &str,
    ) -> Result<Option<MemoryOutboxEvent>, MemoryOutboxError> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let lease_until = (now + ChronoDuration::seconds(LEASE_SECONDS)).to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT event_id, payload_json FROM memory_document_outbox              WHERE payload_json IS NOT NULL AND available_at <= ? AND              (status = 'pending' OR (status = 'processing' AND lease_until <= ?))              ORDER BY created_at ASC, event_id ASC LIMIT 1",
        )
        .bind(&now_text)
        .bind(&now_text)
        .fetch_optional(&mut *transaction)
        .await?;

        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let event_id_text: String = row.try_get("event_id")?;
        let payload: Option<String> = row.try_get("payload_json")?;
        let Some(payload) = payload else {
            transaction.commit().await?;
            return Ok(None);
        };
        let document: MemoryDocument = match serde_json::from_str(&payload) {
            Ok(document) => document,
            Err(_) => {
                sqlx::query(
                    "UPDATE memory_document_outbox SET status = 'dead_letter', payload_json = NULL, \
                     last_error = 'invalid outbox payload', updated_at = ? WHERE event_id = ?",
                )
                .bind(&now_text)
                .bind(&event_id_text)
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                return Ok(None);
            }
        };
        let event_id = Uuid::parse_str(&event_id_text)
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let changed = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'processing', attempts = attempts + 1,              lease_owner = ?, lease_until = ?, updated_at = ?              WHERE event_id = ? AND available_at <= ? AND              (status = 'pending' OR (status = 'processing' AND lease_until <= ?))",
        )
        .bind(worker_id)
        .bind(&lease_until)
        .bind(&now_text)
        .bind(&event_id_text)
        .bind(&now_text)
        .bind(&now_text)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        if changed.rows_affected() != 1 {
            return Ok(None);
        }
        Ok(Some(MemoryOutboxEvent { event_id, document }))
    }

    async fn complete(
        &self,
        event: &MemoryOutboxEvent,
        worker_id: &str,
    ) -> Result<(), MemoryOutboxError> {
        let now = Utc::now().to_rfc3339();
        let version = i64::try_from(event.document.version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let mut transaction = self.pool.begin().await?;
        let completed = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'completed', payload_json = NULL,              lease_owner = NULL, lease_until = NULL, last_error = NULL, updated_at = ?              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(&now)
        .bind(event.event_id.to_string())
        .bind(worker_id)
        .execute(&mut *transaction)
        .await?;
        if completed.rows_affected() != 1 {
            return Err(MemoryOutboxError::LeaseLost);
        }
        let metadata_updated = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored', updated_at = ?              WHERE memory_id = ? AND version = ? AND content_hash = ?",
        )
        .bind(&now)
        .bind(event.document.memory_id.to_string())
        .bind(version)
        .bind(&event.document.content_hash)
        .execute(&mut *transaction)
        .await?;
        if metadata_updated.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn fail(
        &self,
        event_id: Uuid,
        worker_id: &str,
        safe_error: &str,
    ) -> Result<(), MemoryOutboxError> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let row = sqlx::query(
            "SELECT attempts FROM memory_document_outbox              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(event_id.to_string())
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(MemoryOutboxError::LeaseLost);
        };
        let attempts: i64 = row.try_get("attempts")?;
        let dead = attempts >= MAX_ATTEMPTS;
        let delay_seconds = (2_i64.saturating_pow(attempts.min(10) as u32)).min(3600);
        let available_at = (now + ChronoDuration::seconds(delay_seconds)).to_rfc3339();
        let status = if dead { "dead_letter" } else { "pending" };
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE memory_document_outbox SET status = ?, available_at = ?, lease_owner = NULL,              lease_until = NULL, last_error = ?, updated_at = ?              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(status)
        .bind(available_at)
        .bind(safe_error.chars().take(512).collect::<String>())
        .bind(now_text)
        .bind(event_id.to_string())
        .bind(worker_id)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(MemoryOutboxError::LeaseLost);
        }
        if dead {
            sqlx::query(
                "UPDATE memory_document_versions SET sync_status = 'failed', updated_at = ?                  WHERE memory_id = (SELECT memory_id FROM memory_document_outbox WHERE event_id = ?)                  AND version = (SELECT version FROM memory_document_outbox WHERE event_id = ?)",
            )
            .bind(Utc::now().to_rfc3339())
            .bind(event_id.to_string())
            .bind(event_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Approves a proposed document version in the authoritative SQLite
    /// metadata and appends an audit event in the same transaction.
}
