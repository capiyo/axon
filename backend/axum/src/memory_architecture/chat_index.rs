#[derive(Debug, Clone, Deserialize)]
pub struct IndexChatMessageInput {
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub message_id: Uuid,
    pub vector: QoinVector,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
    pub evidence_score: f32,
}
impl MemoryEpochCoordinator {
    /// Index an original chat message. The message row remains authoritative;
    /// the JSON value is a retrieval hydration copy with a stable source ID.
    pub async fn index_chat_message(
        &self,
        input: IndexChatMessageInput,
    ) -> Result<(), sqlx::Error> {
        input
            .vector
            .validate()
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        if !input.evidence_score.is_finite() || !(0.0..=1.0).contains(&input.evidence_score) {
            return Err(sqlx::Error::Protocol(
                "invalid chat-message evidence score".into(),
            ));
        }
        let row = sqlx::query(
            "SELECT m.role, m.content, m.created_at FROM chat_messages m JOIN chats c ON c.id = m.chat_id WHERE c.workspace_id = ? AND m.chat_id = ? AND m.id = ?"
        ).bind(input.workspace_id.to_string()).bind(input.chat_id.to_string())
            .bind(input.message_id.to_string()).fetch_optional(&self.pool).await?
            .ok_or(sqlx::Error::RowNotFound)?;
        let role: String = row.try_get("role")?;
        let content: String = row.try_get("content")?;
        let created_at: String = row.try_get("created_at")?;
        let payload = serde_json::json!({
            "id": input.message_id, "chat_id": input.chat_id, "role": role,
            "content": content, "created_at": created_at, "source_kind": "chat_message"
        })
        .to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO memory_vector_index (memory_id, workspace_id, chat_id, task_id, project_id, owner_agent_id, visibility, status, qoin_vector_json, key_metadata_json, value_payload_json, tags_json, dependency_ids_json, outcome_score, evidence_score, source_event_sequence, model_version, indexed_at, source_kind) VALUES (?, ?, ?, NULL, NULL, NULL, 'workspace_shared', 'approved', ?, ?, ?, ?, ?, 0.5, ?, 0, ?, ?, 'chat_message') ON CONFLICT(memory_id) DO UPDATE SET workspace_id=excluded.workspace_id, chat_id=excluded.chat_id, task_id=NULL, project_id=NULL, owner_agent_id=NULL, visibility='workspace_shared', status='approved', qoin_vector_json=excluded.qoin_vector_json, key_metadata_json=excluded.key_metadata_json, value_payload_json=excluded.value_payload_json, tags_json=excluded.tags_json, dependency_ids_json=excluded.dependency_ids_json, outcome_score=0.5, evidence_score=excluded.evidence_score, source_event_sequence=0, model_version=excluded.model_version, indexed_at=excluded.indexed_at, source_kind='chat_message'"
        ).bind(input.message_id.to_string()).bind(input.workspace_id.to_string())
            .bind(input.chat_id.to_string())
            .bind(serde_json::to_string(&input.vector).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(serde_json::json!({"tags": &input.tags, "dependencies": &input.dependency_ids}).to_string())
            .bind(payload)
            .bind(serde_json::to_string(&input.tags).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(serde_json::to_string(&input.dependency_ids).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
            .bind(input.evidence_score).bind(input.vector.model_version).bind(now)
            .execute(&self.pool).await?;
        Ok(())
    }
}

