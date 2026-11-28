impl ChatMemoryStore {
    pub async fn create_memory_entry(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        actor: &str,
        input: CreateMemoryEntry,
    ) -> Result<Option<MemoryEntry>, sqlx::Error> {
        let now = Utc::now();
        let entry = MemoryEntry {
            id: Uuid::new_v4(),
            chat_id,
            entry_type: input.entry_type,
            text: input.text,
            reason: input.reason,
            source_message_id: input.source_message_id,
            status: "proposed".into(),
            created_at: now,
            approved_at: None,
        };
        let mut tx = self.pool.begin().await?;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM chats WHERE id = ? AND workspace_id = ?")
                .bind(chat_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Ok(None);
        }
        if let Some(source_id) = entry.source_message_id {
            let source_exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM chat_messages WHERE id = ? AND chat_id = ?")
                    .bind(source_id.to_string())
                    .bind(chat_id.to_string())
                    .fetch_optional(&mut *tx)
                    .await?;
            if source_exists.is_none() {
                return Ok(None);
            }
        }
        append_memory_event(
            &mut tx,
            workspace_id,
            chat_id,
            entry.id,
            "created",
            &entry,
            actor,
        )
        .await?;
        bump_memory_version(&mut tx, chat_id, now).await?;
        insert_audit(
            &mut tx,
            workspace_id,
            actor,
            "memory.created",
            "memory_entry",
            entry.id,
            json!({"chat_id": chat_id, "type": entry.entry_type}),
        )
        .await?;
        tx.commit().await?;
        Ok(Some(entry))
    }

    pub async fn mutate_memory_entry(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        entry_id: Uuid,
        actor: &str,
        operation: &str,
        edit: Option<EditMemoryEntry>,
    ) -> Result<Option<MemoryEntry>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut current = replay_memory(&mut *tx, chat_id).await?.remove(&entry_id);
        let Some(mut entry) = current.take() else {
            return Ok(None);
        };
        if entry.status == "deleted" {
            return Ok(None);
        }
        let event_type = match operation {
            "approve" => {
                entry.status = "approved".into();
                entry.approved_at = Some(Utc::now());
                "approved"
            }
            "reject" => {
                entry.status = "rejected".into();
                entry.approved_at = None;
                "rejected"
            }
            "edit" => {
                let Some(edit) = edit else {
                    return Ok(None);
                };
                entry.text = edit.text;
                entry.reason = edit.reason;
                entry.status = "proposed".into();
                entry.approved_at = None;
                "edited"
            }
            "delete" => {
                entry.status = "deleted".into();
                "deleted"
            }
            _ => return Ok(None),
        };
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM chats WHERE id = ? AND workspace_id = ?")
                .bind(chat_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Ok(None);
        }
        append_memory_event(
            &mut tx,
            workspace_id,
            chat_id,
            entry.id,
            event_type,
            &entry,
            actor,
        )
        .await?;
        if entry.status != "approved" {
            let index_status = if entry.status == "superseded" {
                "superseded"
            } else {
                "deleted"
            };
            sqlx::query("UPDATE memory_vector_index SET status = ? WHERE memory_id = ?")
                .bind(index_status)
                .bind(entry.id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        bump_memory_version(&mut tx, chat_id, Utc::now()).await?;
        insert_audit(
            &mut tx,
            workspace_id,
            actor,
            &format!("memory.{event_type}"),
            "memory_entry",
            entry.id,
            json!({"chat_id": chat_id, "status": entry.status}),
        )
        .await?;
        tx.commit().await?;
        Ok(Some(entry))
    }

    async fn rebuild_memory(&self, chat_id: Uuid) -> Result<Vec<MemoryEntry>, sqlx::Error> {
        let mut connection = self.pool.acquire().await?;
        let mut entries = replay_memory(&mut *connection, chat_id)
            .await?
            .into_values()
            .filter(|entry| entry.status != "deleted")
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.created_at);
        Ok(entries)
    }

    async fn regenerate_brief(
        &self,
        chat_id: Uuid,
        workspace_id: Uuid,
        memory_version: i64,
        last_message_id: Option<String>,
        memory: &[MemoryEntry],
        messages: &[ChatMessage],
    ) -> Result<String, sqlx::Error> {
        let approved = memory
            .iter()
            .filter(|entry| entry.status == "approved")
            .collect::<Vec<_>>();
        let recent = messages.iter().rev().take(6).collect::<Vec<_>>();
        let section = |kind: &str| {
            approved
                .iter()
                .filter(|entry| entry.entry_type == kind)
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>()
        };
        let mut lines = Vec::new();
        let where_left_off = recent
            .iter()
            .rev()
            .find(|message| message.role == "user" || message.role == "assistant")
            .map(|message| format!("{}: {}", message.role, message.content))
            .unwrap_or_else(|| "No conversation activity yet.".into());
        lines.push(format!("Where left off: {where_left_off}"));
        let decisions = section("decision");
        lines.push(format!(
            "Decisions: {}",
            if decisions.is_empty() {
                "None recorded".to_owned()
            } else {
                decisions.join("; ")
            }
        ));
        let open_tasks = section("open_task");
        lines.push(format!(
            "Open tasks: {}",
            if open_tasks.is_empty() {
                "None recorded".to_owned()
            } else {
                open_tasks.join("; ")
            }
        ));
        let next = section("note")
            .into_iter()
            .chain(section("constraint"))
            .collect::<Vec<_>>();
        lines.push(format!(
            "Next / constraints: {}",
            if next.is_empty() {
                "No next step recorded".to_owned()
            } else {
                next.join("; ")
            }
        ));
        let facts = section("fact");
        if !facts.is_empty() {
            lines.push(format!("Facts: {}", facts.join("; ")));
        }
        let brief = truncate_words(&lines.join("\\n"), 190);
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO resume_brief_cache (chat_id, memory_version, last_message_id, prompt_version, brief, generated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(chat_id) DO UPDATE SET memory_version = excluded.memory_version, last_message_id = excluded.last_message_id, prompt_version = excluded.prompt_version, brief = excluded.brief, generated_at = excluded.generated_at")
            .bind(chat_id.to_string()).bind(memory_version).bind(last_message_id).bind(RESUME_PROMPT_VERSION).bind(&brief).bind(Utc::now().to_rfc3339()).execute(&mut *tx).await?;
        let _ = workspace_id;
        tx.commit().await?;
        Ok(brief)
    }
}
}
