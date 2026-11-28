    async fn create_workspace_for_user(
        &self,
        user_id: Uuid,
        name: String,
        actor: String,
    ) -> Result<Workspace, StorageError> {
        let mut tx = self.pool.begin().await?;
        let workspace = Workspace {
            id: Uuid::new_v4(),
            name,
            created_at: Utc::now(),
        };
        let entity = StoredEntity::Workspace(workspace.clone());
        sqlx::query("INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) VALUES ('workspace', ?, NULL, ?, ?)")
            .bind(workspace.id.to_string())
            .bind(workspace.created_at.to_rfc3339())
            .bind(serde_json::to_string(&entity)?)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)")
            .bind(workspace.id.to_string())
            .bind(user_id.to_string())
            .bind(workspace.created_at.to_rfc3339())
            .execute(&mut *tx).await?;
        let event = AuditEvent {
            sequence: 0,
            event_id: Uuid::new_v4(),
            workspace_id: Some(workspace.id),
            actor,
            event_type: "workspace.created".into(),
            entity_type: "workspace".into(),
            entity_id: workspace.id.to_string(),
            occurred_at: workspace.created_at,
            metadata: serde_json::json!({"name": workspace.name}),
        };
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(event.event_id.to_string())
            .bind(event.workspace_id.map(|id| id.to_string()))
            .bind(event.actor)
            .bind(event.event_type)
            .bind(event.entity_type)
            .bind(event.entity_id)
            .bind(event.occurred_at.to_rfc3339())
            .bind(serde_json::to_string(&event.metadata)?)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(workspace)
    }
    async fn daily_model_spend(
        &self,
        user_id: Uuid,
        usage_date: &str,
    ) -> Result<u64, StorageError> {
        let spent: Option<i64> = sqlx::query_scalar(
            "SELECT spent_micros FROM model_daily_spend WHERE user_id = ? AND usage_date = ?",
        )
        .bind(user_id.to_string())
        .bind(usage_date)
        .fetch_optional(&self.pool)
        .await?;
        Ok(spent.unwrap_or(0).max(0) as u64)
    }

    async fn reserve_model_spend(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        usage_date: &str,
        amount_micros: u64,
        daily_limit_micros: u64,
    ) -> Result<bool, StorageError> {
        let Ok(amount) = i64::try_from(amount_micros) else {
            return Ok(false);
        };
        let Ok(limit) = i64::try_from(daily_limit_micros) else {
            return Ok(false);
        };
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO model_daily_spend (user_id, usage_date, spent_micros, updated_at) \
             VALUES (?, ?, 0, ?) ON CONFLICT(user_id, usage_date) DO NOTHING",
        )
        .bind(user_id.to_string())
        .bind(usage_date)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        let result = sqlx::query(
            "UPDATE model_daily_spend \
             SET spent_micros = spent_micros + ?, updated_at = ? \
             WHERE user_id = ? AND usage_date = ? AND spent_micros + ? <= ?",
        )
        .bind(amount)
        .bind(&now)
        .bind(user_id.to_string())
        .bind(usage_date)
        .bind(amount)
        .bind(limit)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 1 {
            let event = AuditEvent::attributed(
                format!("human:{user_id}"),
                Some(workspace_id),
                "assignment_budget.reserved",
                "daily_model_budget",
                format!("{user_id}:{usage_date}"),
                serde_json::json!({
                    "reserved_micros": amount_micros,
                    "daily_limit_micros": daily_limit_micros
                }),
            );
            sqlx::query(
                "INSERT INTO audit_events \
                 (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(event.event_id.to_string())
            .bind(event.workspace_id.map(|id| id.to_string()))
            .bind(event.actor)
            .bind(event.event_type)
            .bind(event.entity_type)
            .bind(event.entity_id)
            .bind(event.occurred_at.to_rfc3339())
            .bind(serde_json::to_string(&event.metadata)?)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }
}

