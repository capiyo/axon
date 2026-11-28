#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteStorage;

    pub(super) fn document(workspace_id: Uuid, owner_agent_id: Uuid) -> MemoryDocument {
        let now = Utc::now();
        let mut document = MemoryDocument {
            memory_id: Uuid::new_v4(),
            workspace_id,
            project_id: None,
            task_id: None,
            owner_id: Some(owner_agent_id),
            scope: "agent_private".into(),
            document_type: "technical_decision".into(),
            version: 1,
            title: "Private agent memory".into(),
            content: "Keep this decision in the owning agent scope.".into(),
            content_format: "text/markdown".into(),
            tags: vec!["private".into()],
            source: "test".into(),
            source_kind: "memory_entry".into(),
            source_event_sequence: 0,
            created_at: now,
            updated_at: now,
            content_hash: String::new(),
            key_metadata: serde_json::json!({}),
            value_payload: serde_json::json!({}),
            qkv_version: "axon-lexical-qkv-v1".into(),
            qoin_vector_version: None,
            status: "proposed".into(),
            retention_policy: None,
        };
        document.content_hash = document.calculate_content_hash();
        document
    }

    #[tokio::test]
    async fn outbox_enqueue_is_idempotent_and_approval_is_audited() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut document = document(workspace_id, agent_id);

        outbox.enqueue(&mut document, actor_id).await.unwrap();
        let first_source_sequence = document.source_event_sequence;
        assert!(first_source_sequence > 0);
        outbox.enqueue(&mut document, actor_id).await.unwrap();

        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1);

        assert!(outbox
            .approve(workspace_id, document.memory_id, 1, actor_id)
            .await
            .unwrap());
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE event_type IN ('memory_document.created', 'memory_document.approved') AND entity_id = ?",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(audit_count, 2);

        let stored_sequence: i64 = sqlx::query_scalar(
            "SELECT source_event_sequence FROM memory_document_versions WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(
            u64::try_from(stored_sequence).unwrap(),
            first_source_sequence
        );

        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let visible = outbox
            .list_authorized_document_refs(workspace_id, agent_id, Uuid::nil(), None, None, 20)
            .await
            .unwrap();
        let hidden = outbox
            .list_authorized_document_refs(
                workspace_id,
                Uuid::new_v4(),
                Uuid::nil(),
                None,
                None,
                20,
            )
            .await
            .unwrap();
        assert_eq!(visible, vec![(document.memory_id, 1)]);
        assert!(hidden.is_empty());

        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let pinned = sqlx::query(
            "SELECT document_version, content_hash, source_event_sequence, qkv_version, qoin_scoring_version \
             FROM memory_document_epoch_items WHERE memory_id = ? AND document_version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let pinned_version: i64 = pinned.try_get("document_version").unwrap();
        let pinned_hash: String = pinned.try_get("content_hash").unwrap();
        let pinned_sequence: i64 = pinned.try_get("source_event_sequence").unwrap();
        let pinned_qkv: String = pinned.try_get("qkv_version").unwrap();
        let pinned_qoin: String = pinned.try_get("qoin_scoring_version").unwrap();
        assert_eq!(pinned_version, 1);
        assert_eq!(pinned_hash, document.content_hash);
        assert_eq!(
            u64::try_from(pinned_sequence).unwrap(),
            first_source_sequence
        );
        assert_eq!(pinned_qkv, "axon-lexical-qkv-v1");
        assert_eq!(
            pinned_qoin,
            crate::memory_intelligence::QoinScoringConfig::default().effective_version()
        );
        let update_result = sqlx::query(
            "UPDATE memory_document_epoch_items SET content_hash = 'tampered' WHERE memory_id = ?",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await;
        assert!(update_result.is_err());
        let delete_result =
            sqlx::query("DELETE FROM memory_document_epoch_items WHERE memory_id = ?")
                .bind(document.memory_id.to_string())
                .execute(&outbox.pool)
                .await;
        assert!(delete_result.is_err());
    }
}

#[cfg(test)]
mod epoch_document_tests {
    use super::*;
    use crate::storage::{SqliteStorage, Storage};
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct FakeDocumentRepository {
        documents: Mutex<HashMap<(Uuid, u64), MemoryDocument>>,
    }

    #[async_trait::async_trait]
    impl MemoryDocumentRepository for FakeDocumentRepository {
        async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError> {
            document.validate()?;
            self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .insert((document.memory_id, document.version), document);
            Ok(())
        }

        async fn get_version(
            &self,
            memory_id: Uuid,
            version: u64,
        ) -> Result<Option<MemoryDocument>, MemoryDocumentError> {
            Ok(self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .get(&(memory_id, version))
                .cloned())
        }

        async fn list_versions(
            &self,
            memory_id: Uuid,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut versions = self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|document| document.memory_id == memory_id)
                .cloned()
                .collect::<Vec<_>>();
            versions.sort_by_key(|document| document.version);
            Ok(versions)
        }

        async fn list_workspace_documents(
            &self,
            workspace_id: Uuid,
            limit: usize,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut documents = self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|document| document.workspace_id == workspace_id)
                .cloned()
                .collect::<Vec<_>>();
            documents.sort_by(|left, right| {
                left.memory_id
                    .as_bytes()
                    .cmp(right.memory_id.as_bytes())
                    .then_with(|| left.version.cmp(&right.version))
            });
            documents.truncate(limit.clamp(1, 5_000));
            Ok(documents)
        }

        async fn verify_version(
            &self,
            memory_id: Uuid,
            version: u64,
            expected_hash: &str,
        ) -> Result<bool, MemoryDocumentError> {
            Ok(self
                .get_version(memory_id, version)
                .await?
                .is_some_and(|document| document.content_hash == expected_hash))
        }

        async fn ping(&self) -> Result<(), MemoryDocumentError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn project_scoped_memory_requires_active_project_membership_for_search_and_direct_reads()
    {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let owner = storage
            .ensure_firebase_user("axon-test", "project-memory-owner")
            .await
            .unwrap();
        let member = storage
            .ensure_firebase_user("axon-test", "project-memory-member")
            .await
            .unwrap();
        storage
            .add_workspace_member(owner.workspace.id, owner.user_id, member.user_id)
            .await
            .unwrap();
        let project = storage
            .create_project_for_user(owner.workspace.id, owner.user_id, "Project memory".into())
            .await
            .unwrap();
        storage
            .set_project_member(
                owner.workspace.id,
                project.id,
                owner.user_id,
                member.user_id,
                crate::model::ProjectRole::Member,
            )
            .await
            .unwrap();

        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(owner.workspace.id, agent_id);
        document.scope = "project_shared".into();
        document.project_id = Some(project.id);
        document.owner_id = None;
        document.status = "proposed".into();
        outbox.enqueue(&mut document, owner.user_id).await.unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox
            .approve(owner.workspace.id, document.memory_id, 1, owner.user_id)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let visible = outbox
            .list_authorized_document_refs(
                owner.workspace.id,
                agent_id,
                member.user_id,
                None,
                Some(project.id),
                20,
            )
            .await
            .unwrap();
        assert_eq!(visible, vec![(document.memory_id, 1)]);
        let without_project = outbox
            .list_authorized_document_refs(
                owner.workspace.id,
                agent_id,
                member.user_id,
                None,
                None,
                20,
            )
            .await
            .unwrap();
        assert!(without_project.is_empty());
        let outsider = outbox
            .list_authorized_document_refs(
                owner.workspace.id,
                agent_id,
                Uuid::new_v4(),
                None,
                Some(project.id),
                20,
            )
            .await
            .unwrap();
        assert!(outsider.is_empty());

        let direct = outbox
            .get_authorized_document(
                &repository,
                owner.workspace.id,
                document.memory_id,
                1,
                agent_id,
                member.user_id,
                None,
            )
            .await
            .unwrap();
        assert!(direct.is_some());
        assert!(storage
            .revoke_project_member(
                owner.workspace.id,
                project.id,
                owner.user_id,
                member.user_id,
            )
            .await
            .unwrap());
        let revoked = outbox
            .get_authorized_document(
                &repository,
                owner.workspace.id,
                document.memory_id,
                1,
                agent_id,
                member.user_id,
                None,
            )
            .await
            .unwrap();
        assert!(revoked.is_none());
    }

    #[tokio::test]
    async fn retrieval_feedback_is_session_pinned_authorized_and_audited() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "feedback-owner")
            .await
            .unwrap();
        let agent = crate::model::Agent {
            id: Uuid::new_v4(),
            workspace_id: account.workspace.id,
            created_by_user_id: Some(account.user_id),
            display_name: "Feedback agent".into(),
            provider: "test".into(),
            connection_mode: crate::model::ConnectionMode::Manual,
            created_at: Utc::now(),
        };
        storage
            .apply(crate::storage::StorageMutation {
                entity: crate::storage::StoredEntity::Agent(agent.clone()),
                related_entities: Vec::new(),
                audit: crate::audit::AuditEvent::attributed(
                    format!("human:{}", account.user_id),
                    Some(account.workspace.id),
                    "agent.registered",
                    "agent",
                    agent.id.to_string(),
                    serde_json::json!({}),
                ),
            })
            .await
            .unwrap();

        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let mut document = super::tests::document(account.workspace.id, agent.id);
        document.scope = "workspace_shared".into();
        document.owner_id = None;
        document.status = "proposed".into();
        outbox
            .enqueue(&mut document, account.user_id)
            .await
            .unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox
            .approve(account.workspace.id, document.memory_id, 1, account.user_id)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(
            storage
                .memory_epoch_coordinator()
                .publish_due_epochs()
                .await
                .unwrap(),
            1
        );
        let epoch_id: String = sqlx::query_scalar(
            "SELECT id FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1",
        )
        .bind(account.workspace.id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let session_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_memory_sessions (session_id, workspace_id, agent_id, task_id, memory_epoch_id, initial_memory_ids_json, retrieved_memory_ids_json, created_at, project_id) VALUES (?, ?, ?, NULL, ?, '[]', '[]', ?, NULL)",
        )
        .bind(session_id.to_string())
        .bind(account.workspace.id.to_string())
        .bind(agent.id.to_string())
        .bind(epoch_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let recorded = outbox
            .record_retrieval_feedback(
                account.workspace.id,
                session_id,
                agent.id,
                account.user_id,
                document.memory_id,
                1,
                "helpful",
                &serde_json::json!({"reason": "resolved the task"}),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recorded.event_type, "helpful");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_retrieval_feedback WHERE session_id = ? AND memory_id = ? AND document_version = 1",
        )
        .bind(session_id.to_string())
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        let signals = outbox
            .feedback_signals(account.workspace.id, &[(document.memory_id, 1)])
            .await
            .unwrap();
        let (positive, negative) = signals.get(&(document.memory_id, 1)).copied().unwrap();
        assert!((positive - (2.0 / 3.0)).abs() < 1e-6);
        assert!((negative - (1.0 / 3.0)).abs() < 1e-6);

        let invalid = outbox
            .record_retrieval_feedback(
                account.workspace.id,
                session_id,
                agent.id,
                account.user_id,
                document.memory_id,
                1,
                "made_up_event",
                &serde_json::json!({}),
            )
            .await;
        assert!(matches!(invalid, Err(MemoryOutboxError::VersionConflict)));
        let unpinned = outbox
            .record_retrieval_feedback(
                account.workspace.id,
                session_id,
                agent.id,
                account.user_id,
                Uuid::new_v4(),
                1,
                "helpful",
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        assert!(unpinned.is_none());
    }

    #[tokio::test]
    async fn archive_restore_and_delete_create_new_versions_and_control_current_retrieval() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("axon-test", "lifecycle-user")
            .await
            .unwrap();
        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let mut document = super::tests::document(account.workspace.id, Uuid::new_v4());
        document.scope = "workspace_shared".into();
        document.owner_id = None;
        document.status = "proposed".into();
        document.validate().unwrap();
        outbox
            .enqueue(&mut document, account.user_id)
            .await
            .unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox
            .approve(account.workspace.id, document.memory_id, 1, account.user_id)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(outbox
            .transition_lifecycle(
                &repository,
                account.workspace.id,
                document.memory_id,
                1,
                account.user_id,
                MemoryLifecycleAction::Archive,
            )
            .await
            .unwrap());
        let archived_payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM memory_document_outbox WHERE memory_id = ? AND version = 2",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let archived: MemoryDocument = serde_json::from_str(&archived_payload).unwrap();
        repository.put_version(archived).await.unwrap();
        let hidden = outbox
            .list_authorized_document_refs(
                account.workspace.id,
                Uuid::nil(),
                account.user_id,
                None,
                None,
                20,
            )
            .await
            .unwrap();
        assert!(hidden.is_empty());
        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let archived_epoch_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_document_epoch_items WHERE workspace_id = ?",
        )
        .bind(account.workspace.id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(archived_epoch_count, 0);
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 2",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let direct_archived = outbox
            .get_authorized_document(
                &repository,
                account.workspace.id,
                document.memory_id,
                1,
                Uuid::nil(),
                account.user_id,
                None,
            )
            .await
            .unwrap();
        assert!(direct_archived.is_none());

        assert!(outbox
            .transition_lifecycle(
                &repository,
                account.workspace.id,
                document.memory_id,
                2,
                account.user_id,
                MemoryLifecycleAction::Restore,
            )
            .await
            .unwrap());
        let restored_payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM memory_document_outbox WHERE memory_id = ? AND version = 3",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let restored: MemoryDocument = serde_json::from_str(&restored_payload).unwrap();
        assert_eq!(restored.status, "ready");
        repository.put_version(restored).await.unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 3",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let visible = outbox
            .list_authorized_document_refs(
                account.workspace.id,
                Uuid::nil(),
                account.user_id,
                None,
                None,
                20,
            )
            .await
            .unwrap();
        assert_eq!(visible, vec![(document.memory_id, 3)]);
        // The epoch publisher is hourly; advance its injected clock to the next
        // hourly boundary so the restored version is pinned in a new epoch.
        assert!(coordinator
            .publish_workspace_if_due_at(
                &account.workspace.id.to_string(),
                chrono::Utc::now() + chrono::Duration::hours(1),
            )
            .await
            .unwrap());
        let restored_epoch_version: i64 = sqlx::query_scalar(
            "SELECT i.document_version FROM memory_document_epoch_items i
             JOIN memory_epochs e ON e.id = i.epoch_id
             WHERE i.workspace_id = ? AND i.memory_id = ?
             ORDER BY e.epoch_number DESC LIMIT 1",
        )
        .bind(account.workspace.id.to_string())
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(restored_epoch_version, 3);
        let direct_restored = outbox
            .get_authorized_document(
                &repository,
                account.workspace.id,
                document.memory_id,
                3,
                Uuid::nil(),
                account.user_id,
                None,
            )
            .await
            .unwrap();
        assert!(direct_restored.is_some());

        assert!(outbox
            .transition_lifecycle(
                &repository,
                account.workspace.id,
                document.memory_id,
                3,
                account.user_id,
                MemoryLifecycleAction::Delete,
            )
            .await
            .unwrap());
        let deleted_status: String = sqlx::query_scalar(
            "SELECT lifecycle_status FROM memory_document_versions WHERE memory_id = ? AND version = 4",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(deleted_status, "deleted");
        let deleted_payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM memory_document_outbox WHERE memory_id = ? AND version = 4",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let deleted: MemoryDocument = serde_json::from_str(&deleted_payload).unwrap();
        repository.put_version(deleted).await.unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 4",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        assert!(coordinator
            .publish_workspace_if_due_at(
                &account.workspace.id.to_string(),
                chrono::Utc::now() + chrono::Duration::hours(2),
            )
            .await
            .unwrap());
        let latest_epoch_items: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_document_epoch_items i
             JOIN memory_epochs e ON e.id = i.epoch_id
             WHERE i.workspace_id = ? AND e.epoch_number = (
                 SELECT MAX(epoch_number) FROM memory_epochs WHERE workspace_id = ?
             )",
        )
        .bind(account.workspace.id.to_string())
        .bind(account.workspace.id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(latest_epoch_items, 0);
        let hidden_after_delete = outbox
            .list_authorized_document_refs(
                account.workspace.id,
                Uuid::nil(),
                account.user_id,
                None,
                None,
                20,
            )
            .await
            .unwrap();
        assert!(hidden_after_delete.is_empty());
        let direct_deleted = outbox
            .get_authorized_document(
                &repository,
                account.workspace.id,
                document.memory_id,
                3,
                Uuid::nil(),
                account.user_id,
                None,
            )
            .await
            .unwrap();
        assert!(direct_deleted.is_none());
    }

    #[tokio::test]
    async fn epoch_hydration_returns_only_the_pinned_authorized_document_version() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        let repository = FakeDocumentRepository::default();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox
            .approve(workspace_id, document.memory_id, 1, Uuid::new_v4())
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let coordinator = storage
            .memory_epoch_coordinator()
            .with_qoin_scoring_version("test-qoin-memory-v2".into());
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let epoch_id_text: String = sqlx::query_scalar(
            "SELECT id FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let epoch_id = Uuid::parse_str(&epoch_id_text).unwrap();
        let pinned_scoring_version: String = sqlx::query_scalar(
            "SELECT qoin_scoring_version FROM memory_document_epoch_items WHERE epoch_id = ? LIMIT 1",
        )
        .bind(epoch_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(pinned_scoring_version, "test-qoin-memory-v2");

        let visible = outbox
            .list_epoch_authorized_documents(
                &repository,
                workspace_id,
                epoch_id,
                agent_id,
                Uuid::nil(),
                None,
                20,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].memory_id, document.memory_id);
        assert_eq!(visible[0].version, 1);
        assert_eq!(visible[0].content_hash, document.content_hash);
        assert_eq!(visible[0].status, "ready");

        let hidden = outbox
            .list_epoch_authorized_documents(
                &repository,
                workspace_id,
                epoch_id,
                Uuid::new_v4(),
                Uuid::nil(),
                None,
                20,
            )
            .await
            .unwrap()
            .unwrap();
        assert!(hidden.is_empty());

        let session_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_memory_sessions (session_id, workspace_id, agent_id, task_id, memory_epoch_id, initial_memory_ids_json, retrieved_memory_ids_json, created_at, project_id) VALUES (?, ?, ?, NULL, ?, '[]', '[]', ?, NULL)",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(epoch_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let session_documents = outbox
            .list_session_authorized_documents(
                &repository,
                workspace_id,
                session_id,
                agent_id,
                Uuid::nil(),
                20,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session_documents.0, epoch_id);
        assert!(session_documents.1.is_none());
        assert_eq!(session_documents.2.as_deref(), Some("test-qoin-memory-v2"));
        assert_eq!(session_documents.3.len(), 1);
        assert_eq!(session_documents.3[0].memory_id, document.memory_id);

        let denied_session = outbox
            .list_session_authorized_documents(
                &repository,
                workspace_id,
                session_id,
                Uuid::new_v4(),
                Uuid::nil(),
                20,
            )
            .await
            .unwrap();
        assert!(denied_session.is_none());

        let missing = outbox
            .list_epoch_authorized_documents(
                &repository,
                workspace_id,
                Uuid::new_v4(),
                agent_id,
                Uuid::nil(),
                None,
                20,
            )
            .await
            .unwrap();
        assert!(missing.is_none());
    }
}

#[cfg(test)]
mod outbox_recovery_tests {
    use super::*;
    use crate::storage::SqliteStorage;

    #[tokio::test]
    async fn dead_letter_recovery_requeues_event_and_audits_atomically() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, actor_id).await.unwrap();

        sqlx::query(
            "UPDATE memory_document_outbox SET status = 'dead_letter', attempts = 8 WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'failed' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(outbox
            .retry_dead_letter(workspace_id, document.memory_id, 1, actor_id,)
            .await
            .unwrap());

        let status: String = sqlx::query_scalar(
            "SELECT status FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let attempts: i64 = sqlx::query_scalar(
            "SELECT attempts FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let sync_status: String = sqlx::query_scalar(
            "SELECT sync_status FROM memory_document_versions WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'memory_document.outbox_requeued' AND entity_id = ?",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();

        assert_eq!(status, "pending");
        assert_eq!(attempts, 0);
        assert_eq!(sync_status, "pending");
        assert_eq!(audit_count, 1);
    }

    #[tokio::test]
    async fn malformed_dead_letter_payload_cannot_be_requeued() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        sqlx::query(
            "UPDATE memory_document_outbox SET status = 'dead_letter', payload_json = 'not-json' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'failed' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(!outbox
            .retry_dead_letter(workspace_id, document.memory_id, 1, Uuid::new_v4(),)
            .await
            .unwrap());
    }
}

#[cfg(test)]
mod reconciliation_tests {
    use super::*;
    use crate::storage::SqliteStorage;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct Repository {
        documents: Mutex<HashMap<(Uuid, u64), MemoryDocument>>,
    }

    #[async_trait::async_trait]
    impl MemoryDocumentRepository for Repository {
        async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError> {
            document.validate()?;
            self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .insert((document.memory_id, document.version), document);
            Ok(())
        }

        async fn get_version(
            &self,
            memory_id: Uuid,
            version: u64,
        ) -> Result<Option<MemoryDocument>, MemoryDocumentError> {
            Ok(self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .get(&(memory_id, version))
                .cloned())
        }

        async fn list_versions(
            &self,
            memory_id: Uuid,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut values = self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|item| item.memory_id == memory_id)
                .cloned()
                .collect::<Vec<_>>();
            values.sort_by_key(|item| item.version);
            Ok(values)
        }

        async fn list_workspace_documents(
            &self,
            workspace_id: Uuid,
            limit: usize,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut values = self
                .documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|item| item.workspace_id == workspace_id)
                .cloned()
                .collect::<Vec<_>>();
            values.sort_by(|left, right| {
                left.memory_id
                    .as_bytes()
                    .cmp(right.memory_id.as_bytes())
                    .then_with(|| left.version.cmp(&right.version))
            });
            values.truncate(limit.clamp(1, 5_000));
            Ok(values)
        }

        async fn verify_version(
            &self,
            memory_id: Uuid,
            version: u64,
            expected_hash: &str,
        ) -> Result<bool, MemoryDocumentError> {
            Ok(self
                .get_version(memory_id, version)
                .await?
                .is_some_and(|item| item.content_hash == expected_hash))
        }

        async fn ping(&self) -> Result<(), MemoryDocumentError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn reconciliation_detects_pending_missing_and_metadata_mismatch() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        let repository = Repository::default();
        let pending = outbox
            .reconcile_workspace(&repository, workspace_id, 100)
            .await
            .unwrap();
        assert_eq!(pending.pending_outbox_events, 1);
        assert!(pending
            .issues
            .iter()
            .any(|issue| issue.kind == "mongo_document_pending_delivery"));

        let mut conflicting = document.clone();
        conflicting.scope = "workspace_shared".into();
        conflicting.owner_id = None;
        repository.put_version(conflicting).await.unwrap();
        let mismatch = outbox
            .reconcile_workspace(&repository, workspace_id, 100)
            .await
            .unwrap();
        assert!(mismatch
            .issues
            .iter()
            .any(|issue| issue.kind == "scope_mismatch"));
    }
}

#[cfg(test)]
mod latest_version_search_tests {
    use super::*;
    use crate::storage::SqliteStorage;

    #[tokio::test]
    async fn search_candidate_refs_return_only_latest_approved_synchronized_version() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut first = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut first, actor_id).await.unwrap();
        assert!(outbox
            .approve(workspace_id, first.memory_id, 1, actor_id)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(first.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let mut second = first.clone();
        second.version = 2;
        second.source_event_sequence = 0;
        second.title = "Updated private memory".into();
        second.content = "The newer approved version supersedes the old search candidate.".into();
        second.updated_at = Utc::now();
        second.content_hash = second.calculate_content_hash();
        outbox.enqueue(&mut second, actor_id).await.unwrap();
        assert!(outbox
            .approve(workspace_id, second.memory_id, 2, actor_id)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 2",
        )
        .bind(second.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let refs = outbox
            .list_authorized_document_refs(workspace_id, agent_id, Uuid::nil(), None, None, 200)
            .await
            .unwrap();
        assert_eq!(refs, vec![(first.memory_id, 2)]);

        let first_history_page = outbox
            .list_authorized_document_versions(
                workspace_id,
                first.memory_id,
                agent_id,
                Uuid::nil(),
                None,
                None,
                None,
                1,
            )
            .await
            .unwrap();
        assert_eq!(
            first_history_page
                .iter()
                .map(|item| item.version)
                .collect::<Vec<_>>(),
            vec![2]
        );
        let second_history_page = outbox
            .list_authorized_document_versions(
                workspace_id,
                first.memory_id,
                agent_id,
                Uuid::nil(),
                None,
                None,
                first_history_page.last().map(|item| item.version),
                1,
            )
            .await
            .unwrap();
        assert_eq!(
            second_history_page
                .iter()
                .map(|item| item.version)
                .collect::<Vec<_>>(),
            vec![1]
        );
        let denied_history = outbox
            .list_authorized_document_versions(
                workspace_id,
                first.memory_id,
                Uuid::new_v4(),
                Uuid::nil(),
                None,
                None,
                None,
                100,
            )
            .await
            .unwrap();
        assert!(denied_history.is_empty());

        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let pinned_versions: Vec<i64> = sqlx::query_scalar(
            "SELECT document_version FROM memory_document_epoch_items WHERE memory_id = ? ORDER BY document_version",
        )
        .bind(first.memory_id.to_string())
        .fetch_all(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(pinned_versions, vec![2]);
    }
}
