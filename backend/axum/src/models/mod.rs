//! Domain types grouped by bounded context.
//!
//! Re-exports preserve the historical `crate::models::Type` import path while
//! allowing new code to depend on narrower domain modules.
pub mod agent;
pub mod artifact;
pub mod project;
pub mod task;
pub mod workspace;

pub use agent::*;
pub use artifact::*;
pub use project::*;
pub use task::*;
pub use workspace::*;

#[cfg(test)]
mod agent_ownership_tests {
    use super::*;
    use uuid::Uuid;
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
        serialized
            .as_object_mut()
            .unwrap()
            .remove("created_by_user_id");
        let legacy: Agent = serde_json::from_value(serialized).unwrap();
        assert_eq!(legacy.created_by_user_id, None);
    }

    #[test]
    fn legacy_tasks_deserialize_without_creator_or_source_chat() {
        let legacy = serde_json::json!({
            "id": Uuid::new_v4(),
            "workspace_id": Uuid::new_v4(),
            "title": "Legacy task",
            "objective": "Continue existing work",
            "assignee_agent_id": Uuid::new_v4(),
            "context_artifact_ids": [],
            "response_artifact_id": null,
            "status": "offered",
            "version": 1,
            "created_at": Utc::now(),
            "updated_at": Utc::now(),
            "transitions": []
        });

        let task: Task = serde_json::from_value(legacy).unwrap();
        assert_eq!(task.created_by, Uuid::nil());
        assert_eq!(task.source_chat_id, None);
    }

    #[test]
    fn create_task_accepts_an_optional_source_chat() {
        let chat_id = Uuid::new_v4();
        let input: CreateTask = serde_json::from_value(serde_json::json!({
            "title": "Continue this conversation",
            "objective": "Use approved memory and recent context",
            "assignee_agent_id": Uuid::new_v4(),
            "source_chat_id": chat_id
        }))
        .unwrap();

        assert_eq!(input.source_chat_id, Some(chat_id));
        assert!(input.context_artifact_ids.is_empty());
    }
}
