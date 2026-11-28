impl MemoryOutbox {
    pub async fn record_retrieval_feedback(
        &self,
        workspace_id: Uuid,
        session_id: Uuid,
        agent_id: Uuid,
        actor_user_id: Uuid,
        memory_id: Uuid,
        document_version: u64,
        event_type: &str,
        evidence: &Value,
    ) -> Result<Option<RecordedMemoryFeedback>, MemoryOutboxError> {
        const ALLOWED_EVENTS: &[&str] = &[
            "retrieved",
            "selected",
            "ignored",
            "reused",
            "helpful",
            "irrelevant",
            "contradicted",
            "incorrect",
        ];
        if !ALLOWED_EVENTS.contains(&event_type) {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let evidence_json = serde_json::to_string(evidence)?;
        if evidence_json.len() > 4096 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let version_sql = i64::try_from(document_version)
            .map_err(|_| sqlx::Error::Protocol("feedback version exceeds SQLite range".into()))?;
        let now = Utc::now();
        let feedback_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        let workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if workspace_member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let agent_payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(agent_payload) = agent_payload else {
            tx.rollback().await?;
            return Ok(None);
        };
        let agent_entity: crate::storage::StoredEntity = serde_json::from_str(&agent_payload)?;
        if !matches!(agent_entity, crate::storage::StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(actor_user_id))
        {
            tx.rollback().await?;
            return Ok(None);
        }
        let session = sqlx::query(
            "SELECT memory_epoch_id, task_id FROM agent_memory_sessions
             WHERE session_id = ? AND workspace_id = ? AND agent_id = ?
               AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(session) = session else {
            tx.rollback().await?;
            return Ok(None);
        };
        let epoch_id: String = session.try_get("memory_epoch_id")?;
        let session_task_id: Option<String> = session.try_get("task_id")?;
        let pinned = sqlx::query(
            "SELECT scope, project_id, owner_id, task_id FROM memory_document_epoch_items
             WHERE epoch_id = ? AND workspace_id = ? AND memory_id = ? AND document_version = ?",
        )
        .bind(epoch_id)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(pinned) = pinned else {
            tx.rollback().await?;
            return Ok(None);
        };
        let scope: String = pinned.try_get("scope")?;
        let project_id: Option<String> = pinned.try_get("project_id")?;
        let owner_id: Option<String> = pinned.try_get("owner_id")?;
        let task_id: Option<String> = pinned.try_get("task_id")?;
        let agent_id_text = agent_id.to_string();
        match scope.as_str() {
            "workspace_shared" => {}
            "agent_private" if owner_id.as_deref() == Some(agent_id_text.as_str()) => {}
            "task_shared" if task_id.is_some() && task_id == session_task_id => {}
            "project_shared" => {
                let Some(project_id) = project_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let membership: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM project_memberships pm
                     JOIN projects p ON p.id = pm.project_id
                     JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                     WHERE pm.project_id = ? AND pm.user_id = ?
                       AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                )
                .bind(project_id)
                .bind(actor_user_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
                if membership.is_none() {
                    tx.rollback().await?;
                    return Ok(None);
                }
            }
            _ => {
                tx.rollback().await?;
                return Ok(None);
            }
        }
        sqlx::query(
            "INSERT INTO memory_retrieval_feedback
             (feedback_id, workspace_id, session_id, agent_id, memory_id, document_version,
              actor_user_id, event_type, created_at, evidence_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(feedback_id.to_string())
        .bind(workspace_id.to_string())
        .bind(session_id.to_string())
        .bind(agent_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .bind(actor_user_id.to_string())
        .bind(event_type)
        .bind(now.to_rfc3339())
        .bind(evidence_json)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json)
             VALUES (?, ?, ?, 'memory.retrieval_feedback_recorded', 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(memory_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({
            "feedback_id": feedback_id,
            "session_id": session_id,
            "agent_id": agent_id,
            "document_version": document_version,
            "event_type": event_type
        }).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(RecordedMemoryFeedback {
            feedback_id,
            memory_id,
            document_version,
            event_type: event_type.to_owned(),
            created_at: now,
        }))
    }

    /// Computes smoothed positive/negative feedback signals for exact
    /// document versions. Unrated versions are omitted so their existing
    /// metadata scores remain unchanged.
    pub async fn feedback_signals(
        &self,
        workspace_id: Uuid,
        versions: &[(Uuid, u64)],
    ) -> Result<HashMap<(Uuid, u64), (f32, f32)>, MemoryOutboxError> {
        if versions.is_empty() {
            return Ok(HashMap::new());
        }
        let normalized_versions = versions
            .iter()
            .map(|(memory_id, version)| {
                i64::try_from(*version)
                    .map(|version| (memory_id.to_string(), version))
                    .map_err(|_| {
                        sqlx::Error::Protocol("feedback version exceeds SQLite range".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT memory_id, document_version, event_type, COUNT(*) AS event_count
             FROM memory_retrieval_feedback WHERE workspace_id = ",
        );
        query.push_bind(workspace_id.to_string());
        query.push(" AND (memory_id, document_version) IN ");
        query.push_tuples(
            normalized_versions.iter(),
            |mut row, (memory_id, version)| {
                row.push_bind(memory_id).push_bind(*version);
            },
        );
        query.push(" GROUP BY memory_id, document_version, event_type");
        let rows = query.build().fetch_all(&self.pool).await?;
        let mut counts: HashMap<(Uuid, u64), (u64, u64)> = HashMap::new();
        for row in rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_raw: i64 = row.try_get("document_version")?;
            let version = u64::try_from(version_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let event_type: String = row.try_get("event_type")?;
            let count_raw: i64 = row.try_get("event_count")?;
            let count = u64::try_from(count_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            match event_type.as_str() {
                "helpful" | "selected" | "reused" => {
                    let entry = counts.entry((memory_id, version)).or_default();
                    entry.0 = entry.0.saturating_add(count);
                }
                "irrelevant" | "contradicted" | "incorrect" => {
                    let entry = counts.entry((memory_id, version)).or_default();
                    entry.1 = entry.1.saturating_add(count);
                }
                _ => {}
            }
        }
        Ok(counts
            .into_iter()
            .map(|(key, (positive, negative))| {
                let total = (positive + negative) as f32;
                let positive_signal = (positive as f32 + 1.0) / (total + 2.0);
                let negative_signal = (negative as f32 + 1.0) / (total + 2.0);
                (key, (positive_signal, negative_signal))
            })
            .collect())
    }

    /// Returns only approved, synchronized versions visible to this
    /// workspace-owned agent. The query runs against SQLite before MongoDB
    /// content is hydrated by the API layer.
    #[allow(clippy::too_many_arguments)]
}
