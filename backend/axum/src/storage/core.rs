    async fn ping(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    fn chat_memory_store(&self) -> ChatMemoryStore {
        ChatMemoryStore::new(self.pool.clone())
    }

    fn memory_epoch_coordinator(&self) -> MemoryEpochCoordinator {
        MemoryEpochCoordinator::new(self.pool.clone())
    }

    async fn apply(&self, mutation: StorageMutation) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        for entity in std::iter::once(mutation.entity).chain(mutation.related_entities) {
            let payload = serde_json::to_string(&entity)?;
            let workspace_id = entity.workspace_id().map(|id| id.to_string());
            let created_at = entity.created_at().to_rfc3339();
            sqlx::query(
                "INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(kind, id) DO UPDATE SET \
                 workspace_id = excluded.workspace_id, \
                 created_at = excluded.created_at, \
                 payload_json = excluded.payload_json",
            )
            .bind(entity.kind().as_str())
            .bind(entity.id().to_string())
            .bind(workspace_id)
            .bind(created_at)
            .bind(payload)
            .execute(&mut *transaction)
            .await?;
        }

        let audit = mutation.audit;
        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(audit.event_id.to_string())
        .bind(audit.workspace_id.map(|id| id.to_string()))
        .bind(audit.actor)
        .bind(audit.event_type)
        .bind(audit.entity_type)
        .bind(audit.entity_id)
        .bind(audit.occurred_at.to_rfc3339())
        .bind(serde_json::to_string(&audit.metadata)?)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    async fn claim_task_execution(
        &self,
        task_id: Uuid,
        owner_token: Uuid,
        lease_seconds: u64,
    ) -> Result<bool, StorageError> {
        let now = Utc::now();
        let lease_seconds = i64::try_from(lease_seconds.clamp(30, 600)).unwrap_or(240);
        let expires_at = now + ChronoDuration::seconds(lease_seconds);
        let now_text = now.to_rfc3339();
        let mut transaction = self.pool.begin().await?;

        sqlx::query("DELETE FROM task_execution_leases WHERE lease_expires_at <= ?")
            .bind(&now_text)
            .execute(&mut *transaction)
            .await?;

        let result = sqlx::query(
            "INSERT INTO task_execution_leases (task_id, owner_token, lease_expires_at, created_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(task_id) DO UPDATE SET \
                 owner_token = excluded.owner_token, \
                 lease_expires_at = excluded.lease_expires_at, \
                 created_at = excluded.created_at \
             WHERE task_execution_leases.lease_expires_at <= ?",
        )
        .bind(task_id.to_string())
        .bind(owner_token.to_string())
        .bind(expires_at.to_rfc3339())
        .bind(&now_text)
        .bind(&now_text)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    async fn release_task_execution(
        &self,
        task_id: Uuid,
        owner_token: Uuid,
    ) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM task_execution_leases WHERE task_id = ? AND owner_token = ?")
            .bind(task_id.to_string())
            .bind(owner_token.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_entities(
        &self,
        kind: EntityKind,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<StoredEntity>, StorageError> {
        let workspace_id = workspace_id.map(|id| id.to_string());
        let rows = sqlx::query(
            "SELECT payload_json FROM entities \
             WHERE kind = ? AND (? IS NULL OR workspace_id = ?) \
             ORDER BY created_at ASC, id ASC",
        )
        .bind(kind.as_str())
        .bind(workspace_id.as_deref())
        .bind(workspace_id.as_deref())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                Ok(serde_json::from_str(&payload)?)
            })
            .collect()
    }

    async fn list_audit_events(&self, workspace_id: Uuid) -> Result<Vec<AuditEvent>, StorageError> {
        let rows = sqlx::query(
            "SELECT sequence, event_id, workspace_id, actor, event_type, entity_type, \
             entity_id, occurred_at, metadata_json FROM audit_events \
             WHERE workspace_id = ? ORDER BY sequence ASC",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let sequence: i64 = row.try_get("sequence")?;
                let event_id: String = row.try_get("event_id")?;
                let workspace_id: Option<String> = row.try_get("workspace_id")?;
                let occurred_at: String = row.try_get("occurred_at")?;
                let metadata: String = row.try_get("metadata_json")?;
                Ok(AuditEvent {
                    sequence: u64::try_from(sequence)
                        .map_err(|_| StorageError::InvalidStoredSequence)?,
                    event_id: Uuid::parse_str(&event_id)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    workspace_id: workspace_id
                        .map(|value| Uuid::parse_str(&value))
                        .transpose()
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    actor: row.try_get("actor")?,
                    event_type: row.try_get("event_type")?,
                    entity_type: row.try_get("entity_type")?,
                    entity_id: row.try_get("entity_id")?,
                    occurred_at: DateTime::parse_from_rfc3339(&occurred_at)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?
                        .with_timezone(&Utc),
                    metadata: serde_json::from_str(&metadata)?,
                })
            })
            .collect()
    }

