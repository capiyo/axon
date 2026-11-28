use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

const RESUME_PROMPT_VERSION: &str = "resume-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived: bool,
    pub unread_count: i64,
    pub is_liked: bool,
    pub message_count: i64,
    pub last_message: Option<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub answered_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub chat_id: Uuid,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub text: String,
    pub reason: Option<String>,
    pub source_message_id: Option<Uuid>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatDetail {
    pub chat: Chat,
    pub messages: Vec<ChatMessage>,
    pub memory: Vec<MemoryEntry>,
    pub resume_brief: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateChat {
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMessage {
    pub role: String,
    pub content: String,
    pub answered_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMemoryEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    pub text: String,
    pub reason: Option<String>,
    pub source_message_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EditMemoryEntry {
    pub text: String,
    pub reason: Option<String>,
}

#[derive(Clone)]
pub struct ChatMemoryStore {
    pool: SqlitePool,
}

