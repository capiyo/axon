impl ChatMemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_chats(&self, workspace_id: Uuid) -> Result<Vec<Chat>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, workspace_id, owner_id, title, created_at, updated_at, archived FROM chats WHERE workspace_id = ? AND archived = 0 ORDER BY updated_at DESC, id DESC")
            .bind(workspace_id.to_string()).fetch_all(&self.pool).await?;
        rows.iter().map(parse_chat).collect()
    }

    pub async fn create_chat(
        &self,
        workspace_id: Uuid,
        owner_id: Uuid,
        actor: &str,
        title: String,
    ) -> Result<Chat, sqlx::Error> {
        let now = Utc::now();
        let chat = Chat {
            id: Uuid::new_v4(),
            workspace_id,
            owner_id,
            title,
            created_at: now,
            updated_at: now,
            archived: false,
            unread_count: 0,
            is_liked: false,
            message_count: 0,
            last_message: None,
        };
        let mut tx = self.pool.begin().await?;
        let is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if is_member.is_none() {
            return Err(sqlx::Error::RowNotFound);
        }
        sqlx::query("INSERT INTO chats (id, workspace_id, owner_id, title, created_at, updated_at, archived, memory_version) VALUES (?, ?, ?, ?, ?, ?, 0, 0)")
            .bind(chat.id.to_string()).bind(workspace_id.to_string()).bind(owner_id.to_string()).bind(&chat.title)
            .bind(now.to_rfc3339()).bind(now.to_rfc3339()).execute(&mut *tx).await?;
        insert_audit(
            &mut tx,
            workspace_id,
            actor,
            "chat.created",
            "chat",
            chat.id,
            json!({"chat_id": chat.id}),
        )
        .await?;
        tx.commit().await?;
        Ok(chat)
    }

    pub async fn list_chats_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<Chat>, sqlx::Error> {
        let mut chats = self.list_chats(workspace_id).await?;
        for chat in &mut chats {
            self.enrich_chat(chat, user_id).await?;
        }
        Ok(chats)
    }

    pub async fn get_chat_for_user(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<ChatDetail>, sqlx::Error> {
        let mut detail = self.get_chat(workspace_id, chat_id).await?;
        if let Some(detail) = detail.as_mut() {
            self.enrich_chat(&mut detail.chat, user_id).await?;
        }
        Ok(detail)
    }

    async fn enrich_chat(&self, chat: &mut Chat, user_id: Uuid) -> Result<(), sqlx::Error> {
        chat.message_count =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE chat_id = ?")
                .bind(chat.id.to_string())
                .fetch_one(&self.pool)
                .await?;
        let last_message = sqlx::query(
            "SELECT id, chat_id, role, content, created_at, answered_by FROM chat_messages WHERE chat_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(chat.id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        chat.last_message = last_message.as_ref().map(parse_message).transpose()?;
        chat.unread_count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_messages m LEFT JOIN chat_read_states r ON r.chat_id = m.chat_id AND r.user_id = ? WHERE m.chat_id = ? AND m.role = 'assistant' AND m.created_at > COALESCE(r.last_read_at, '')",
        )
        .bind(user_id.to_string())
        .bind(chat.id.to_string())
        .fetch_one(&self.pool)
        .await?;
        let liked: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chat_reactions WHERE chat_id = ? AND user_id = ? AND reaction = 'like'",
        )
        .bind(chat.id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        chat.is_liked = liked.is_some();
        Ok(())
    }

    pub async fn mark_chat_read(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "INSERT INTO chat_read_states (chat_id, user_id, last_read_at) SELECT id, ?, ? FROM chats WHERE id = ? AND workspace_id = ? ON CONFLICT(chat_id, user_id) DO UPDATE SET last_read_at = excluded.last_read_at",
        )
        .bind(user_id.to_string())
        .bind(now)
        .bind(chat_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn toggle_chat_like(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        user_id: Uuid,
        actor: &str,
    ) -> Result<Option<bool>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chats WHERE id = ? AND workspace_id = ? AND archived = 0",
        )
        .bind(chat_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Ok(None);
        }
        let liked: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chat_reactions WHERE chat_id = ? AND user_id = ? AND reaction = 'like'",
        )
        .bind(chat_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let is_liked = if liked.is_some() {
            sqlx::query("DELETE FROM chat_reactions WHERE chat_id = ? AND user_id = ? AND reaction = 'like'")
                .bind(chat_id.to_string())
                .bind(user_id.to_string())
                .execute(&mut *tx)
                .await?;
            false
        } else {
            sqlx::query("INSERT INTO chat_reactions (chat_id, user_id, reaction, created_at) VALUES (?, ?, 'like', ?)")
                .bind(chat_id.to_string())
                .bind(user_id.to_string())
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
            true
        };
        insert_audit(
            &mut tx,
            workspace_id,
            actor,
            if is_liked {
                "chat.liked"
            } else {
                "chat.unliked"
            },
            "chat",
            chat_id,
            json!({"is_liked": is_liked}),
        )
        .await?;
        tx.commit().await?;
        Ok(Some(is_liked))
    }

    pub async fn get_chat(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
    ) -> Result<Option<ChatDetail>, sqlx::Error> {
        let row = sqlx::query("SELECT id, workspace_id, owner_id, title, created_at, updated_at, archived, memory_version FROM chats WHERE id = ? AND workspace_id = ?")
            .bind(chat_id.to_string()).bind(workspace_id.to_string()).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let chat = parse_chat(&row)?;
        let memory_version: i64 = row.try_get("memory_version")?;
        let messages = self.list_messages(chat_id).await?;
        let memory = self.rebuild_memory(chat_id).await?;
        let last_message_id = messages.last().map(|message| message.id.to_string());
        let cached = sqlx::query("SELECT memory_version, last_message_id, prompt_version, brief FROM resume_brief_cache WHERE chat_id = ?")
            .bind(chat_id.to_string()).fetch_optional(&self.pool).await?;
        let brief = if let Some(cached) = cached {
            let cached_version: i64 = cached.try_get("memory_version")?;
            let cached_last: Option<String> = cached.try_get("last_message_id")?;
            let cached_prompt: String = cached.try_get("prompt_version")?;
            if cached_version == memory_version
                && cached_last == last_message_id
                && cached_prompt == RESUME_PROMPT_VERSION
            {
                cached.try_get("brief")?
            } else {
                self.regenerate_brief(
                    chat_id,
                    workspace_id,
                    memory_version,
                    last_message_id,
                    &memory,
                    &messages,
                )
                .await?
            }
        } else {
            self.regenerate_brief(
                chat_id,
                workspace_id,
                memory_version,
                last_message_id,
                &memory,
                &messages,
            )
            .await?
        };
        Ok(Some(ChatDetail {
            chat,
            messages,
            memory,
            resume_brief: brief,
        }))
    }

    async fn list_messages(&self, chat_id: Uuid) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, chat_id, role, content, created_at, answered_by FROM chat_messages WHERE chat_id = ? ORDER BY created_at ASC, id ASC")
            .bind(chat_id.to_string()).fetch_all(&self.pool).await?;
        rows.iter().map(parse_message).collect()
    }

    pub async fn append_message(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        actor: &str,
        input: CreateMessage,
    ) -> Result<Option<ChatMessage>, sqlx::Error> {
        let now = Utc::now();
        let message = ChatMessage {
            id: Uuid::new_v4(),
            chat_id,
            role: input.role,
            content: input.content,
            created_at: now,
            answered_by: input.answered_by,
        };
        let mut tx = self.pool.begin().await?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM chats WHERE id = ? AND workspace_id = ? AND archived = 0",
        )
        .bind(chat_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Ok(None);
        }
        sqlx::query("INSERT INTO chat_messages (id, chat_id, role, content, created_at, answered_by) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(message.id.to_string()).bind(chat_id.to_string()).bind(&message.role).bind(&message.content)
            .bind(now.to_rfc3339()).bind(&message.answered_by).execute(&mut *tx).await?;
        sqlx::query("UPDATE chats SET updated_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(chat_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM resume_brief_cache WHERE chat_id = ?")
            .bind(chat_id.to_string())
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            workspace_id,
            actor,
            "chat.message_created",
            "chat_message",
            message.id,
            json!({"chat_id": chat_id, "role": message.role}),
        )
        .await?;
        tx.commit().await?;
        Ok(Some(message))
    }

    pub async fn archive_chat(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        actor: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("UPDATE chats SET archived = 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND archived = 0")
            .bind(Utc::now().to_rfc3339()).bind(chat_id.to_string()).bind(workspace_id.to_string()).execute(&mut *tx).await?;
        if result.rows_affected() == 1 {
            insert_audit(
                &mut tx,
                workspace_id,
                actor,
                "chat.archived",
                "chat",
                chat_id,
                json!({"chat_id": chat_id}),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

}
