use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// Original human creator; default keeps previously persisted task records readable.
    #[serde(default)]
    pub created_by: Uuid,
    pub title: String,
    pub objective: String,
    pub assignee_agent_id: Uuid,
    /// Optional source conversation used as bounded, untrusted task context.
    #[serde(default)]
    pub source_chat_id: Option<Uuid>,
    pub context_artifact_ids: Vec<Uuid>,
    pub response_artifact_id: Option<Uuid>,
    pub status: TaskStatus,
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub transitions: Vec<TaskTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Offered,
    Acknowledged,
    InProgress,
    Submitted,
    ReviewRequired,
    Accepted,
    ChangesRequested,
    Completed,
    Rejected,
    Cancelled,
}

#[derive(Debug, Deserialize)]
pub struct CreateTask {
    pub title: String,
    pub objective: String,
    pub assignee_agent_id: Uuid,
    #[serde(default)]
    pub source_chat_id: Option<Uuid>,
    #[serde(default)]
    pub context_artifact_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitTask {
    pub expected_version: u64,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct TransitionTask {
    pub expected_version: u64,
    pub status: TaskStatus,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTransition {
    pub from: Option<TaskStatus>,
    pub to: TaskStatus,
    pub note: String,
    pub occurred_at: DateTime<Utc>,
}
