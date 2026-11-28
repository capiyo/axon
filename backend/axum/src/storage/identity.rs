    async fn ensure_google_user(&self, subject: &str, email: &str, display_name: Option<&str>, avatar_url: Option<&str>) -> Result<UserWorkspace, StorageError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now();
        let candidate_user_id = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, auth_provider, provider_subject, created_at, email, display_name, avatar_url) VALUES (?, 'google', ?, ?, ?, ?, ?) ON CONFLICT(auth_provider, provider_subject) DO NOTHING")
            .bind(candidate_user_id.to_string()).bind(subject).bind(now.to_rfc3339()).bind(email).bind(display_name).bind(avatar_url).execute(&mut *tx).await?;
        sqlx::query("UPDATE users SET email = ?, display_name = COALESCE(?, display_name), avatar_url = COALESCE(?, avatar_url) WHERE auth_provider = 'google' AND provider_subject = ?")
            .bind(email).bind(display_name).bind(avatar_url).bind(subject).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT id, personal_workspace_id FROM users WHERE auth_provider = 'google' AND provider_subject = ?").bind(subject).fetch_one(&mut *tx).await?;
        let user_id_text: String = row.try_get("id")?;
        let user_id = Uuid::parse_str(&user_id_text).map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let linked_workspace: Option<String> = row.try_get("personal_workspace_id")?;
        let workspace = if let Some(workspace_id) = linked_workspace {
            let workspace_id = Uuid::parse_str(&workspace_id).map_err(|_| StorageError::InvalidStoredIdentifier)?;
            let row = sqlx::query("SELECT payload_json FROM entities WHERE kind = 'workspace' AND id = ?").bind(workspace_id.to_string()).fetch_one(&mut *tx).await?;
            let payload: String = row.try_get("payload_json")?;
            match serde_json::from_str::<StoredEntity>(&payload)? { StoredEntity::Workspace(workspace) => workspace, _ => return Err(StorageError::InvalidStoredEntityKind) }
        } else {
            let workspace = Workspace { id: Uuid::new_v4(), name: "Personal workspace".into(), created_at: now };
            let payload = serde_json::to_string(&StoredEntity::Workspace(workspace.clone()))?;
            sqlx::query("INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) VALUES ('workspace', ?, NULL, ?, ?)").bind(workspace.id.to_string()).bind(workspace.created_at.to_rfc3339()).bind(payload).execute(&mut *tx).await?;
            sqlx::query("UPDATE users SET personal_workspace_id = ? WHERE id = ? AND personal_workspace_id IS NULL").bind(workspace.id.to_string()).bind(user_id.to_string()).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?) ON CONFLICT(workspace_id, user_id) DO NOTHING").bind(workspace.id.to_string()).bind(user_id.to_string()).bind(now.to_rfc3339()).execute(&mut *tx).await?;
            let event = AuditEvent { sequence: 0, event_id: Uuid::new_v4(), workspace_id: Some(workspace.id), actor: format!("human:{user_id}"), event_type: "workspace.personal_created".into(), entity_type: "workspace".into(), entity_id: workspace.id.to_string(), occurred_at: now, metadata: serde_json::json!({"provisioned_by":"google_oauth"}) };
            sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)").bind(event.event_id.to_string()).bind(event.workspace_id.map(|id| id.to_string())).bind(event.actor).bind(event.event_type).bind(event.entity_type).bind(event.entity_id).bind(event.occurred_at.to_rfc3339()).bind(serde_json::to_string(&event.metadata)?).execute(&mut *tx).await?;
            workspace
        };
        tx.commit().await?;
        Ok(UserWorkspace { user_id, workspace })
    }
    async fn is_workspace_member(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE user_id = ? AND workspace_id = ?",
        )
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn list_user_workspaces(&self, user_id: Uuid) -> Result<Vec<Workspace>, StorageError> {
        let rows = sqlx::query(
            "SELECT e.payload_json FROM workspace_memberships m JOIN entities e ON e.kind = 'workspace' AND e.id = m.workspace_id WHERE m.user_id = ? ORDER BY m.created_at ASC"
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                match serde_json::from_str::<StoredEntity>(&payload)? {
                    StoredEntity::Workspace(workspace) => Ok(workspace),
                    _ => Err(StorageError::InvalidStoredEntityKind),
                }
            })
            .collect()
    }

