    async fn add_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<WorkspaceMembership, StorageError> {
        let mut tx = self.pool.begin().await?;
        let actor_is_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_is_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id = ?")
            .bind(target_user_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if target_exists.is_none() {
            return Err(StorageError::ProjectWorkspaceMembershipRequired);
        }
        let now = Utc::now();
        let inserted = sqlx::query(
            "INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'member', ?) ON CONFLICT(workspace_id, user_id) DO NOTHING",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT role, created_at FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let role: String = row.try_get("role")?;
        let created_at: String = row.try_get("created_at")?;
        if inserted.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'workspace.member_added', 'workspace', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(workspace_id.to_string())
            .bind(now.to_rfc3339())
            .bind(serde_json::json!({"user_id": target_user_id, "role": role}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(WorkspaceMembership {
            workspace_id,
            user_id: target_user_id,
            role,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map_err(|_| StorageError::InvalidStoredTimestamp)?
                .with_timezone(&Utc),
        })
    }

    async fn revoke_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool.begin().await?;
        let actor_is_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_is_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let changed = sqlx::query(
            "DELETE FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'member'",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() == 1 {
            let now = Utc::now().to_rfc3339();
            sqlx::query(
                "UPDATE project_memberships SET revoked_at = ? WHERE user_id = ? AND project_id IN (SELECT id FROM projects WHERE workspace_id = ?) AND revoked_at IS NULL AND role != 'owner'",
            )
            .bind(&now)
            .bind(target_user_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE agent_credentials SET revoked_at = ? WHERE owner_user_id = ? AND workspace_id = ? AND revoked_at IS NULL",
            )
            .bind(&now)
            .bind(target_user_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'workspace.member_revoked', 'workspace', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(workspace_id.to_string())
            .bind(now)
            .bind(serde_json::json!({"user_id": target_user_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(changed.rows_affected() == 1)
    }

    async fn create_project_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        name: String,
    ) -> Result<Project, StorageError> {
        let mut tx = self.pool.begin().await?;
        let workspace_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if workspace_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let project = Project {
            id: Uuid::new_v4(),
            workspace_id,
            name,
            created_by_user_id: user_id,
            created_at: Utc::now(),
        };
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, created_by_user_id, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project.id.to_string())
        .bind(workspace_id.to_string())
        .bind(&project.name)
        .bind(user_id.to_string())
        .bind(project.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
        )
        .bind(project.id.to_string())
        .bind(user_id.to_string())
        .bind(project.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.created', 'project', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{user_id}"))
        .bind(project.id.to_string())
        .bind(project.created_at.to_rfc3339())
        .bind(serde_json::json!({"name": project.name}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(project)
    }

    async fn list_user_projects(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<Project>, StorageError> {
        let rows = sqlx::query(
            "SELECT p.id, p.workspace_id, p.name, p.created_by_user_id, p.created_at
             FROM projects p
             JOIN project_memberships m ON m.project_id = p.id
             JOIN workspace_memberships w ON w.workspace_id = p.workspace_id AND w.user_id = m.user_id
             WHERE p.workspace_id = ? AND m.user_id = ? AND m.revoked_at IS NULL
             ORDER BY p.created_at ASC, p.id ASC",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let workspace: String = row.try_get("workspace_id")?;
                let creator: String = row.try_get("created_by_user_id")?;
                let created_at: String = row.try_get("created_at")?;
                Ok(Project {
                    id: Uuid::parse_str(&id).map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    workspace_id: Uuid::parse_str(&workspace)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    name: row.try_get("name")?,
                    created_by_user_id: Uuid::parse_str(&creator)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?
                        .with_timezone(&Utc),
                })
            })
            .collect()
    }

    async fn is_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM projects p
             JOIN project_memberships m ON m.project_id = p.id
             JOIN workspace_memberships w ON w.workspace_id = p.workspace_id AND w.user_id = m.user_id
             WHERE p.id = ? AND p.workspace_id = ? AND m.user_id = ? AND m.revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn set_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
        role: ProjectRole,
    ) -> Result<ProjectMembership, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project = sqlx::query("SELECT id FROM projects WHERE id = ? AND workspace_id = ?")
            .bind(project_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if project.is_none() {
            return Err(StorageError::ProjectNotFound);
        }
        let actor_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_workspace_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let actor_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(actor_role) = actor_role else {
            return Err(StorageError::ProjectAuthorizationDenied);
        };
        if actor_role != "owner" && actor_role != "admin" {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        if role != ProjectRole::Member && role != ProjectRole::Viewer && actor_role != "owner" {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_role.as_deref() == Some("owner") && role != ProjectRole::Owner {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        if actor_role == "admin" && matches!(target_role.as_deref(), Some("owner" | "admin")) {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_is_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_is_workspace_member.is_none() {
            return Err(StorageError::ProjectWorkspaceMembershipRequired);
        }
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role, created_at, revoked_at)
             VALUES (?, ?, ?, ?, NULL)
             ON CONFLICT(project_id, user_id) DO UPDATE SET role = excluded.role, revoked_at = NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .bind(role.as_str())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.member_set', 'project', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(project_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"user_id": target_user_id, "role": role.as_str()}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ProjectMembership {
            project_id,
            user_id: target_user_id,
            role,
            created_at: now,
        })
    }

    async fn revoke_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ? AND workspace_id = ?")
                .bind(project_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if project_exists.is_none() {
            return Err(StorageError::ProjectNotFound);
        }
        let actor_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_workspace_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let actor_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if !matches!(actor_role.as_deref(), Some("owner" | "admin")) {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_role.as_deref() == Some("owner")
            || (actor_role.as_deref() == Some("admin") && target_role.as_deref() == Some("admin"))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let now = Utc::now().to_rfc3339();
        let changed = sqlx::query(
            "UPDATE project_memberships SET revoked_at = ? WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL AND role != 'owner'",
        )
        .bind(&now)
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.member_revoked', 'project', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(project_id.to_string())
            .bind(&now)
            .bind(serde_json::json!({"user_id": target_user_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(changed.rows_affected() == 1)
    }

