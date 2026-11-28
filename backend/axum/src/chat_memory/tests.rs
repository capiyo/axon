#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SqliteStorage, Storage};

    #[tokio::test]
    async fn resume_brief_uses_only_approved_memory_and_invalidates_after_mutation() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "memory-test-user-1")
            .await
            .unwrap();
        let store = storage.chat_memory_store();
        let workspace_id = account.workspace.id;
        let chat = store
            .create_chat(
                workspace_id,
                account.user_id,
                "human:test",
                "Persistence design".into(),
            )
            .await
            .unwrap();
        let message = store
            .append_message(
                workspace_id,
                chat.id,
                "human:test",
                CreateMessage {
                    role: "user".into(),
                    content: "We need a durable way to continue this project.".into(),
                    answered_by: None,
                },
            )
            .await
            .unwrap()
            .unwrap();

        let proposed = store
            .create_memory_entry(
                workspace_id,
                chat.id,
                "human:test",
                CreateMemoryEntry {
                    entry_type: "decision".into(),
                    text: "Use append-only memory events as the source of truth.".into(),
                    reason: Some("Can be rebuilt after cache loss.".into()),
                    source_message_id: Some(message.id),
                },
            )
            .await
            .unwrap()
            .unwrap();
        let before_approval = store
            .get_chat(workspace_id, chat.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!before_approval
            .resume_brief
            .contains("append-only memory events"));
        assert!(before_approval
            .resume_brief
            .contains("Decisions: None recorded"));
        assert!(before_approval
            .resume_brief
            .contains("Open tasks: None recorded"));
        assert!(before_approval
            .resume_brief
            .contains("Next / constraints: No next step recorded"));

        store
            .mutate_memory_entry(
                workspace_id,
                chat.id,
                proposed.id,
                "human:test",
                "approve",
                None,
            )
            .await
            .unwrap()
            .unwrap();
        let after_approval = store
            .get_chat(workspace_id, chat.id)
            .await
            .unwrap()
            .unwrap();
        assert!(after_approval.resume_brief.contains("Decisions:"));
        assert!(after_approval
            .resume_brief
            .contains("append-only memory events"));

        store
            .mutate_memory_entry(
                workspace_id,
                chat.id,
                proposed.id,
                "human:test",
                "edit",
                Some(EditMemoryEntry {
                    text: "Use SQLite as the durable event store.".into(),
                    reason: None,
                }),
            )
            .await
            .unwrap()
            .unwrap();
        let after_edit = store
            .get_chat(workspace_id, chat.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!after_edit
            .resume_brief
            .contains("append-only memory events"));
        assert!(!after_edit
            .resume_brief
            .contains("Use SQLite as the durable event store"));
        assert_eq!(after_edit.memory[0].status, "proposed");

        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_events WHERE chat_id = ?")
                .bind(chat.id.to_string())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(event_count, 3);

        let audit_payloads: Vec<String> =
            sqlx::query_scalar("SELECT metadata_json FROM audit_events WHERE workspace_id = ?")
                .bind(workspace_id.to_string())
                .fetch_all(&store.pool)
                .await
                .unwrap();
        assert!(audit_payloads
            .iter()
            .all(|payload| !payload.contains("durable way to continue")));
    }

    #[tokio::test]
    async fn memory_event_and_audit_write_share_one_transaction() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "memory-atomicity-user")
            .await
            .unwrap();
        let store = storage.chat_memory_store();
        let workspace_id = account.workspace.id;
        let chat = store
            .create_chat(
                workspace_id,
                account.user_id,
                "human:test",
                "Atomic memory writes".into(),
            )
            .await
            .unwrap();

        sqlx::query("CREATE TRIGGER fail_memory_audit BEFORE INSERT ON audit_events WHEN NEW.event_type = 'memory.created' BEGIN SELECT RAISE(ABORT, 'test audit failure'); END")
            .execute(&store.pool)
            .await
            .unwrap();
        let failed = store
            .create_memory_entry(
                workspace_id,
                chat.id,
                "human:test",
                CreateMemoryEntry {
                    entry_type: "note".into(),
                    text: "This event must roll back with its audit failure.".into(),
                    reason: None,
                    source_message_id: None,
                },
            )
            .await;
        assert!(failed.is_err());

        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_events WHERE chat_id = ?")
                .bind(chat.id.to_string())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(
            event_count, 0,
            "memory event and audit write share one transaction"
        );

        sqlx::query("DROP TRIGGER fail_memory_audit")
            .execute(&store.pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn chat_reads_are_workspace_scoped_and_archived_chats_disappear_from_list() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "memory-test-user")
            .await
            .unwrap();
        let store = storage.chat_memory_store();
        let workspace_id = account.workspace.id;
        let other_workspace = Uuid::new_v4();
        let chat = store
            .create_chat(
                workspace_id,
                account.user_id,
                "human:test",
                "Private chat".into(),
            )
            .await
            .unwrap();

        assert!(store
            .get_chat(other_workspace, chat.id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.list_chats(workspace_id).await.unwrap().len(), 1);
        assert!(store
            .archive_chat(workspace_id, chat.id, "human:test")
            .await
            .unwrap());
        assert!(store.list_chats(workspace_id).await.unwrap().is_empty());
        assert!(store
            .get_chat(workspace_id, chat.id)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn chat_summary_tracks_messages_reads_and_likes_per_user() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "chat-summary-user")
            .await
            .unwrap();
        let store = storage.chat_memory_store();
        let chat = store
            .create_chat(
                account.workspace.id,
                account.user_id,
                "human:test",
                "Summary test".into(),
            )
            .await
            .unwrap();
        store
            .append_message(
                account.workspace.id,
                chat.id,
                "human:test",
                CreateMessage {
                    role: "assistant".into(),
                    content: "A reply to count as unread.".into(),
                    answered_by: Some("test-agent".into()),
                },
            )
            .await
            .unwrap()
            .unwrap();

        let summary = store
            .list_chats_for_user(account.workspace.id, account.user_id)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.unread_count, 1);
        assert!(!summary.is_liked);
        assert_eq!(
            summary
                .last_message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("A reply to count as unread.")
        );

        assert!(store
            .mark_chat_read(account.workspace.id, chat.id, account.user_id)
            .await
            .unwrap());
        assert_eq!(
            store
                .list_chats_for_user(account.workspace.id, account.user_id)
                .await
                .unwrap()[0]
                .unread_count,
            0
        );
        assert_eq!(
            store
                .toggle_chat_like(account.workspace.id, chat.id, account.user_id, "human:test")
                .await
                .unwrap(),
            Some(true)
        );
        assert!(
            store
                .list_chats_for_user(account.workspace.id, account.user_id)
                .await
                .unwrap()[0]
                .is_liked
        );
        assert_eq!(
            store
                .toggle_chat_like(account.workspace.id, chat.id, account.user_id, "human:test")
                .await
                .unwrap(),
            Some(false)
        );
    }

    #[tokio::test]
    async fn memory_event_log_rejects_updates_and_deletes() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "memory-test-user-3")
            .await
            .unwrap();
        let store = storage.chat_memory_store();
        let workspace_id = account.workspace.id;
        let chat = store
            .create_chat(
                workspace_id,
                account.user_id,
                "human:test",
                "Append only".into(),
            )
            .await
            .unwrap();
        let entry = store
            .create_memory_entry(
                workspace_id,
                chat.id,
                "human:test",
                CreateMemoryEntry {
                    entry_type: "fact".into(),
                    text: "Events are immutable.".into(),
                    reason: None,
                    source_message_id: None,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let update =
            sqlx::query("UPDATE memory_events SET actor = 'tampered' WHERE memory_entry_id = ?")
                .bind(entry.id.to_string())
                .execute(&store.pool)
                .await;
        assert!(update.is_err());
        let delete = sqlx::query("DELETE FROM memory_events WHERE memory_entry_id = ?")
            .bind(entry.id.to_string())
            .execute(&store.pool)
            .await;
        assert!(delete.is_err());
    }
}

