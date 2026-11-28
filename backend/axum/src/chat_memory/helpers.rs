fn truncate_words(value: &str, limit: usize) -> String {
    let words = value.split_whitespace().take(limit).collect::<Vec<_>>();
    words.join(" ")
}

fn parse_chat(row: &sqlx::sqlite::SqliteRow) -> Result<Chat, sqlx::Error> {
    Ok(Chat {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        workspace_id: Uuid::parse_str(&row.try_get::<String, _>("workspace_id")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        owner_id: Uuid::parse_str(&row.try_get::<String, _>("owner_id")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        title: row.try_get("title")?,
        created_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
            .with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("updated_at")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
            .with_timezone(&Utc),
        archived: row.try_get::<i64, _>("archived")? != 0,
        unread_count: 0,
        is_liked: false,
        message_count: 0,
        last_message: None,
    })
}

fn parse_message(row: &sqlx::sqlite::SqliteRow) -> Result<ChatMessage, sqlx::Error> {
    Ok(ChatMessage {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        chat_id: Uuid::parse_str(&row.try_get::<String, _>("chat_id")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        role: row.try_get("role")?,
        content: row.try_get("content")?,
        created_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
            .with_timezone(&Utc),
        answered_by: row.try_get("answered_by")?,
    })
}

async fn replay_memory<'a, E>(
    executor: E,
    chat_id: Uuid,
) -> Result<HashMap<Uuid, MemoryEntry>, sqlx::Error>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(
        "SELECT payload_json FROM memory_events WHERE chat_id = ? ORDER BY sequence ASC",
    )
    .bind(chat_id.to_string())
    .fetch_all(executor)
    .await?;
    let mut entries = HashMap::new();
    for row in rows {
        let payload: String = row.try_get("payload_json")?;
        let entry: MemoryEntry =
            serde_json::from_str(&payload).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        entries.insert(entry.id, entry);
    }
    Ok(entries)
}

async fn append_memory_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: Uuid,
    chat_id: Uuid,
    entry_id: Uuid,
    event_type: &str,
    entry: &MemoryEntry,
    actor: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO memory_events (event_id, workspace_id, chat_id, memory_entry_id, event_type, payload_json, actor, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(workspace_id.to_string()).bind(chat_id.to_string()).bind(entry_id.to_string()).bind(event_type)
        .bind(serde_json::to_string(entry).map_err(|e| sqlx::Error::Encode(Box::new(e)))?).bind(actor).bind(Utc::now().to_rfc3339())
        .execute(&mut **tx).await?;
    Ok(())
}

async fn bump_memory_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    chat_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE chats SET memory_version = memory_version + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.to_rfc3339())
    .bind(chat_id.to_string())
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM resume_brief_cache WHERE chat_id = ?")
        .bind(chat_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: Uuid,
    actor: &str,
    event_type: &str,
    entity_type: &str,
    entity_id: Uuid,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(workspace_id.to_string()).bind(actor).bind(event_type).bind(entity_type).bind(entity_id.to_string()).bind(Utc::now().to_rfc3339()).bind(metadata.to_string())
        .execute(&mut **tx).await?;
    Ok(())
}

