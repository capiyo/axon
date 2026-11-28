#[cfg(test)]
mod chat_index_tests {
    use super::*;
    use crate::{
        chat_memory::CreateMessage,
        storage::{SqliteStorage, Storage},
    };

    #[tokio::test]
    async fn original_chat_messages_are_snapshotted_and_retrievable() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-chat-index-test", "chat-index-user")
            .await
            .unwrap();
        let chats = storage.chat_memory_store();
        let chat = chats
            .create_chat(
                account.workspace.id,
                account.user_id,
                "test",
                "Raw history".into(),
            )
            .await
            .unwrap();
        let message = chats
            .append_message(
                account.workspace.id,
                chat.id,
                "test",
                CreateMessage {
                    role: "user".into(),
                    content: "The deployment failed after the migration.".into(),
                    answered_by: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let coordinator = storage.memory_epoch_coordinator();
        coordinator
            .index_chat_message(IndexChatMessageInput {
                workspace_id: account.workspace.id,
                chat_id: chat.id,
                message_id: message.id,
                vector: QoinVector {
                    values: vec![1.0, 0.0],
                    model_version: "qoin-chat-v1".into(),
                },
                tags: BTreeSet::from(["deployment".to_owned()]),
                dependency_ids: BTreeSet::new(),
                evidence_score: 1.0,
            })
            .await
            .unwrap();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let handover = coordinator
            .prepare_handover(HandoverRequest {
                workspace_id: account.workspace.id,
                agent_id: Uuid::new_v4(),
                task_id: None,
                project_id: None,
                query: QoinVector {
                    values: vec![1.0, 0.0],
                    model_version: "qoin-chat-v1".into(),
                },
                heads: 1,
                tags: BTreeSet::from(["deployment".to_owned()]),
                dependency_ids: BTreeSet::new(),
                pareto: ParetoPolicy::default(),
            })
            .await
            .unwrap();
        assert_eq!(handover.initial.len(), 1);
        assert_eq!(handover.initial[0].memory_id, message.id);
        assert_eq!(handover.initial[0].source_kind, "chat_message");
        assert!(handover.initial[0].payload_json.contains("migration"));
    }
}

