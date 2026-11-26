use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspace {
    pub name: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMembership {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AddWorkspaceMember {
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub created_by_user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Owner,
    Admin,
    Member,
    Viewer,
}

impl ProjectRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SetProjectMember {
    pub user_id: Uuid,
    pub role: ProjectRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMembership {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub role: ProjectRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub content: String,
    pub source_agent_id: Option<Uuid>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateArtifact {
    pub title: String,
    pub content: String,
    pub source_agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub objective: String,
    pub assignee_agent_id: Uuid,
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

#[cfg(test)]
mod agent_ownership_tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn legacy_agent_records_deserialize_without_owner_metadata() {
        let agent = Agent {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            created_by_user_id: Some(Uuid::new_v4()),
            display_name: "Agent".into(),
            provider: "manual".into(),
            connection_mode: ConnectionMode::Manual,
            created_at: Utc::now(),
        };
        let mut serialized = serde_json::to_value(agent).unwrap();
        serialized.as_object_mut().unwrap().remove("created_by_user_id");
        let legacy: Agent = serde_json::from_value(serialized).unwrap();
        assert_eq!(legacy.created_by_user_id, None);
    }
}
