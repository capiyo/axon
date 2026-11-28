#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::{
        chat_memory::{ChatMemoryStore, CreateMemoryEntry},
        storage::{SqliteStorage, Storage},
    };

    #[tokio::test]
    async fn hourly_epoch_pins_pareto_handover_and_progressive_context() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-memory-test", "epoch-user")
            .await
            .unwrap();
        let chats: ChatMemoryStore = storage.chat_memory_store();
        let chat = chats
            .create_chat(
                account.workspace.id,
                account.user_id,
                "test",
                "Memory".into(),
            )
            .await
            .unwrap();
        let first = chats
            .create_memory_entry(
                account.workspace.id,
                chat.id,
                "test",
                CreateMemoryEntry {
                    entry_type: "decision".into(),
                    text: "Use immutable event snapshots.".into(),
                    reason: None,
                    source_message_id: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let second = chats
            .create_memory_entry(
                account.workspace.id,
                chat.id,
                "test",
                CreateMemoryEntry {
                    entry_type: "fact".into(),
                    text: "A secondary historical detail.".into(),
                    reason: None,
                    source_message_id: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        for entry in [&first, &second] {
            chats
                .mutate_memory_entry(
                    account.workspace.id,
                    chat.id,
                    entry.id,
                    "test",
                    "approve",
                    None,
                )
                .await
                .unwrap()
                .unwrap();
        }
        let coordinator = storage.memory_epoch_coordinator();
        for (entry, vector) in [
            (&first, vec![1.0, 0.0, 1.0, 0.0]),
            (&second, vec![-1.0, 0.0, -1.0, 0.0]),
        ] {
            coordinator
                .index_approved_memory(IndexMemoryInput {
                    workspace_id: account.workspace.id,
                    chat_id: chat.id,
                    memory_id: entry.id,
                    task_id: None,
                    project_id: None,
                    owner_agent_id: None,
                    visibility: "workspace_shared".into(),
                    vector: QoinVector {
                        values: vector,
                        model_version: "qoin-v1".into(),
                    },
                    tags: BTreeSet::new(),
                    dependency_ids: BTreeSet::new(),
                    outcome_score: if entry.id == first.id { 0.9 } else { 0.0 },
                    evidence_score: if entry.id == first.id { 0.9 } else { 0.0 },
                })
                .await
                .unwrap();
        }
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        // Re-indexing after publication must not rewrite the already-published
        // epoch. New agents still read the frozen vector until the next epoch.
        coordinator
            .index_approved_memory(IndexMemoryInput {
                workspace_id: account.workspace.id,
                chat_id: chat.id,
                memory_id: first.id,
                task_id: None,
                project_id: None,
                owner_agent_id: None,
                visibility: "workspace_shared".into(),
                vector: QoinVector {
                    values: vec![-1.0, 0.0, -1.0, 0.0],
                    model_version: "qoin-v1".into(),
                },
                tags: BTreeSet::new(),
                dependency_ids: BTreeSet::new(),
                outcome_score: 0.0,
                evidence_score: 0.0,
            })
            .await
            .unwrap();
        let agent_id = Uuid::new_v4();
        let handover = coordinator
            .prepare_handover(HandoverRequest {
                workspace_id: account.workspace.id,
                agent_id,
                task_id: None,
                project_id: None,
                query: QoinVector {
                    values: vec![1.0, 0.0, 1.0, 0.0],
                    model_version: "qoin-v1".into(),
                },
                heads: 2,
                tags: BTreeSet::new(),
                dependency_ids: BTreeSet::new(),
                pareto: ParetoPolicy::default(),
            })
            .await
            .unwrap();
        assert_eq!(handover.initial.len(), 1);
        assert_eq!(handover.initial[0].memory_id, first.id);
        assert_eq!(handover.deferred_count, 1);
        assert!(!handover.budget_exhausted);

        let progressive = coordinator
            .retrieve_progressive(
                account.workspace.id,
                agent_id,
                handover.session_id,
                ProgressiveRetrievalRequest {
                    query: QoinVector {
                        values: vec![1.0, 0.0, 1.0, 0.0],
                        model_version: "qoin-v1".into(),
                    },
                    heads: 2,
                    tags: BTreeSet::new(),
                    dependency_ids: BTreeSet::new(),
                    limit: 5,
                },
            )
            .await
            .unwrap();
        assert_eq!(progressive.memories.len(), 1);
        assert_eq!(progressive.memories[0].memory_id, second.id);
        assert_eq!(progressive.remaining_candidates, 0);
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn proposed_memory_cannot_enter_the_qoin_index() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-memory-test", "proposed-user")
            .await
            .unwrap();
        let chats = storage.chat_memory_store();
        let chat = chats
            .create_chat(
                account.workspace.id,
                account.user_id,
                "test",
                "Proposed".into(),
            )
            .await
            .unwrap();
        let entry = chats
            .create_memory_entry(
                account.workspace.id,
                chat.id,
                "test",
                CreateMemoryEntry {
                    entry_type: "note".into(),
                    text: "Not approved yet.".into(),
                    reason: None,
                    source_message_id: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let result = storage
            .memory_epoch_coordinator()
            .index_approved_memory(IndexMemoryInput {
                workspace_id: account.workspace.id,
                chat_id: chat.id,
                memory_id: entry.id,
                task_id: None,
                project_id: None,
                owner_agent_id: None,
                visibility: "workspace_shared".into(),
                vector: QoinVector {
                    values: vec![1.0, 0.0],
                    model_version: "qoin-v1".into(),
                },
                tags: BTreeSet::new(),
                dependency_ids: BTreeSet::new(),
                outcome_score: 0.5,
                evidence_score: 0.5,
            })
            .await;
        assert!(result.is_err());
    }
}

