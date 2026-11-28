use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub workspace_id: Uuid,
    #[serde(default)]
    pub created_by_user_id: Option<Uuid>,
    pub display_name: String,
    pub provider: String,
    pub connection_mode: ConnectionMode,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMode {
    Manual,
}

#[derive(Debug, Deserialize)]
pub struct RegisterAgent {
    pub display_name: String,
    pub provider: String,
}
