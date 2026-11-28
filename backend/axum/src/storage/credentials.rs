    async fn issue_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<IssuedAgentCredential, StorageError> {
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if owner_is_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = payload else {
            return Err(StorageError::ProjectNotFound);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.id == agent_id && agent.workspace_id == workspace_id
                && agent.created_by_user_id == Some(owner_user_id))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }

        let credential_id = Uuid::new_v4();
        let secret = Uuid::new_v4().simple().to_string();
        let token = format!("axn_{credential_id}.{secret}");
        let now = Utc::now();
        let expires_at = now + ChronoDuration::days(90);
        sqlx::query(
            "INSERT INTO agent_credentials (id, workspace_id, agent_id, owner_user_id, secret_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_issued', 'agent', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{owner_user_id}"))
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"credential_id": credential_id, "expires_at": expires_at}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(IssuedAgentCredential {
            credential_id,
            token,
            created_at: now,
            expires_at,
        })
    }

    async fn rotate_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<IssuedAgentCredential>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if owner_is_member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let owner_is_agent_owner: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = owner_is_agent_owner else {
            return Ok(None);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(owner_user_id))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let now = Utc::now();
        let revoked = sqlx::query(
            "UPDATE agent_credentials SET revoked_at = ? WHERE id = ? AND workspace_id = ? AND agent_id = ? AND owner_user_id = ? AND revoked_at IS NULL",
        )
        .bind(now.to_rfc3339())
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if revoked.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }
        let new_id = Uuid::new_v4();
        let secret = Uuid::new_v4().simple().to_string();
        let token = format!("axn_{new_id}.{secret}");
        let expires_at = now + ChronoDuration::days(90);
        sqlx::query(
            "INSERT INTO agent_credentials (id, workspace_id, agent_id, owner_user_id, secret_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_rotated', 'agent', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{owner_user_id}"))
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"revoked_credential_id": credential_id, "credential_id": new_id}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(IssuedAgentCredential {
            credential_id: new_id,
            token,
            created_at: now,
            expires_at,
        }))
    }

    async fn revoke_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let agent_payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let is_owner = agent_payload
            .map(|payload| serde_json::from_str::<StoredEntity>(&payload))
            .transpose()?
            .is_some_and(|entity| {
                matches!(entity, StoredEntity::Agent(ref agent)
                if agent.created_by_user_id == Some(owner_user_id))
            });
        if owner_is_member.is_none() || !is_owner {
            tx.rollback().await?;
            return Ok(false);
        }
        let revoked = sqlx::query(
            "UPDATE agent_credentials SET revoked_at = ? WHERE id = ? AND workspace_id = ? AND agent_id = ? AND owner_user_id = ? AND revoked_at IS NULL",
        )
        .bind(&now)
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if revoked.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_revoked', 'agent', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{owner_user_id}"))
            .bind(agent_id.to_string())
            .bind(&now)
            .bind(serde_json::json!({"credential_id": credential_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(revoked.rows_affected() == 1)
    }

    async fn authenticate_agent_credential(
        &self,
        token: &str,
    ) -> Result<Option<AgentCredentialIdentity>, StorageError> {
        let Some((credential_id, secret)) = parse_agent_credential_token(token) else {
            return Ok(None);
        };
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT c.agent_id, c.workspace_id, c.owner_user_id,
                    u.provider_subject, u.firebase_project_id, u.personal_workspace_id
             FROM agent_credentials c
             JOIN users u ON u.id = c.owner_user_id
             WHERE c.id = ? AND c.secret_hash = ? AND c.revoked_at IS NULL AND c.expires_at > ?",
        )
        .bind(credential_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let agent_id_text: String = row.try_get("agent_id")?;
        let workspace_id_text: String = row.try_get("workspace_id")?;
        let owner_user_id_text: String = row.try_get("owner_user_id")?;
        let firebase_uid: String = row.try_get("provider_subject")?;
        let firebase_project_id: Option<String> = row.try_get("firebase_project_id")?;
        let personal_workspace_id: Option<String> = row.try_get("personal_workspace_id")?;
        let agent_id =
            Uuid::parse_str(&agent_id_text).map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let workspace_id = Uuid::parse_str(&workspace_id_text)
            .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let owner_user_id = Uuid::parse_str(&owner_user_id_text)
            .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let firebase_project_id =
            firebase_project_id.ok_or(StorageError::InvalidStoredIdentifier)?;
        let personal_workspace_id = Uuid::parse_str(
            personal_workspace_id
                .as_deref()
                .ok_or(StorageError::InvalidStoredIdentifier)?,
        )
        .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = payload else {
            tx.rollback().await?;
            return Ok(None);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(owner_user_id))
        {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query("UPDATE agent_credentials SET last_used_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(credential_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(AgentCredentialIdentity {
            credential_id,
            agent_id,
            workspace_id,
            owner_user_id,
            firebase_uid,
            firebase_project_id,
            personal_workspace_id,
        }))
    }

