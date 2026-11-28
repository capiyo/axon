#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;
    use serde_json::json;

    fn mutation(workspace_id: Uuid, event_id: Uuid) -> StorageMutation {
        let now = Utc::now();
        StorageMutation {
            entity: StoredEntity::Workspace(Workspace {
                id: workspace_id,
                name: "Axon".into(),
                created_at: now,
            }),
            related_entities: Vec::new(),
            audit: AuditEvent {
                sequence: 0,
                event_id,
                workspace_id: Some(workspace_id),
                actor: "human:test-user".into(),
                event_type: "workspace.created".into(),
                entity_type: "workspace".into(),
                entity_id: workspace_id.to_string(),
                occurred_at: now,
                metadata: json!({"name": "Axon"}),
            },
        }
    }

    #[tokio::test]
    async fn sqlite_persists_entities_and_audit_events() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let workspace_id = Uuid::new_v4();
        storage
            .apply(mutation(workspace_id, Uuid::new_v4()))
            .await
            .unwrap();

        let entities = storage
            .list_entities(EntityKind::Workspace, None)
            .await
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert!(matches!(&entities[0], StoredEntity::Workspace(item) if item.id == workspace_id));

        let events = storage.list_audit_events(workspace_id).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[0].actor, "human:test-user");
    }

    #[tokio::test]
    async fn entity_mutation_rolls_back_when_audit_insert_fails() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let first_workspace = Uuid::new_v4();
        let duplicate_event = Uuid::new_v4();
        storage
            .apply(mutation(first_workspace, duplicate_event))
            .await
            .unwrap();

        let second_workspace = Uuid::new_v4();
        assert!(storage
            .apply(mutation(second_workspace, duplicate_event))
            .await
            .is_err());

        let entities = storage
            .list_entities(EntityKind::Workspace, None)
            .await
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert!(
            matches!(&entities[0], StoredEntity::Workspace(item) if item.id == first_workspace)
        );
        assert!(storage
            .list_audit_events(second_workspace)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn audit_events_reject_update_and_delete() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let workspace_id = Uuid::new_v4();
        storage
            .apply(mutation(workspace_id, Uuid::new_v4()))
            .await
            .unwrap();

        let update =
            sqlx::query("UPDATE audit_events SET actor = 'changed' WHERE workspace_id = ?")
                .bind(workspace_id.to_string())
                .execute(&storage.pool)
                .await;
        assert!(update.is_err());

        let delete = sqlx::query("DELETE FROM audit_events WHERE workspace_id = ?")
            .bind(workspace_id.to_string())
            .execute(&storage.pool)
            .await;
        assert!(delete.is_err());
    }

    #[tokio::test]
    async fn google_first_login_is_idempotent_under_concurrent_requests() {
        let database_path =
            std::env::temp_dir().join(format!("axon-identity-{}.db", Uuid::new_v4()));
        let database_url = format!("sqlite://{}", database_path.display());
        let storage = SqliteStorage::connect(&database_url).await.unwrap();
        let (first, second) = tokio::join!(
            storage.ensure_google_user("same-google-sub", "same-firebase-uid@example.com", None, None),
            storage.ensure_google_user("same-firebase-uid", "same-firebase-uid@example.com", None, None),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.user_id, second.user_id);
        assert_eq!(first.workspace.id, second.workspace.id);
        assert!(storage
            .is_workspace_member(first.user_id, first.workspace.id)
            .await
            .unwrap());
        assert_eq!(
            storage
                .list_user_workspaces(first.user_id)
                .await
                .unwrap()
                .len(),
            1
        );
        let audit = storage.list_audit_events(first.workspace.id).await.unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].actor, format!("human:{}", first.user_id));
        drop(storage);
        std::fs::remove_file(database_path).unwrap();
    }

    #[tokio::test]
    async fn workspace_membership_requires_an_existing_workspace_entity() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_google_user("membership-integrity-user", "membership-integrity-user@example.com", None, None)
            .await
            .unwrap();
        let missing_workspace_id = Uuid::new_v4();

        let result = sqlx::query(
            "INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'member', ?)",
        )
        .bind(missing_workspace_id.to_string())
        .bind(account.user_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&storage.pool)
        .await;

        assert!(result.is_err());
        assert!(!storage
            .is_workspace_member(account.user_id, missing_workspace_id)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn task_execution_lease_is_exclusive_and_owner_scoped() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let task_id = Uuid::new_v4();
        let first_owner = Uuid::new_v4();
        let second_owner = Uuid::new_v4();

        assert!(storage
            .claim_task_execution(task_id, first_owner, 60)
            .await
            .unwrap());
        assert!(!storage
            .claim_task_execution(task_id, second_owner, 60)
            .await
            .unwrap());

        storage
            .release_task_execution(task_id, second_owner)
            .await
            .unwrap();
        assert!(!storage
            .claim_task_execution(task_id, second_owner, 60)
            .await
            .unwrap());

        storage
            .release_task_execution(task_id, first_owner)
            .await
            .unwrap();
        assert!(storage
            .claim_task_execution(task_id, second_owner, 60)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn daily_model_budget_reservations_are_persisted_and_enforced() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();

        assert_eq!(
            storage
                .daily_model_spend(user_id, "2026-11-28")
                .await
                .unwrap(),
            0
        );
        assert!(storage
            .reserve_model_spend(user_id, workspace_id, "2026-11-28", 60, 100)
            .await
            .unwrap());
        assert!(!storage
            .reserve_model_spend(user_id, workspace_id, "2026-11-28", 50, 100)
            .await
            .unwrap());
        assert_eq!(
            storage
                .daily_model_spend(user_id, "2026-11-28")
                .await
                .unwrap(),
            60
        );
        assert!(!storage
            .reserve_model_spend(user_id, workspace_id, "2026-11-28", u64::MAX, 100)
            .await
            .unwrap());
        let audit = storage.list_audit_events(workspace_id).await.unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].event_type, "assignment_budget.reserved");
    }

    #[tokio::test]
    async fn supervisor_runs_round_trip_through_durable_entity_storage() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let workspace_id = Uuid::new_v4();
        let run = SupervisorRun::new(
            workspace_id,
            "Persisted run".into(),
            "Verify durable state".into(),
        )
        .unwrap();

        storage
            .apply(StorageMutation {
                entity: StoredEntity::SupervisorRun(run.clone()),
                related_entities: Vec::new(),
                audit: AuditEvent::attributed(
                    "human:test",
                    Some(workspace_id),
                    "supervisor_run.created",
                    "supervisor_run",
                    run.id.to_string(),
                    serde_json::json!({"title": run.title}),
                ),
            })
            .await
            .unwrap();

        let stored = storage
            .list_entities(EntityKind::SupervisorRun, Some(workspace_id))
            .await
            .unwrap();
        assert!(
            matches!(stored.as_slice(), [StoredEntity::SupervisorRun(item)] if item.id == run.id)
        );
    }
    #[tokio::test]
    async fn personal_workspace_ids_are_unique_across_users() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let first = storage
            .ensure_google_user("personal-workspace-owner-a", "personal-workspace-owner-a@example.com", None, None)
            .await
            .unwrap();
        let second = storage
            .ensure_google_user("personal-workspace-owner-b", "personal-workspace-owner-b@example.com", None, None)
            .await
            .unwrap();
        assert_ne!(first.workspace.id, second.workspace.id);

        let duplicate = sqlx::query("UPDATE users SET personal_workspace_id = ? WHERE id = ?")
            .bind(first.workspace.id.to_string())
            .bind(second.user_id.to_string())
            .execute(&storage.pool)
            .await;
        assert!(
            duplicate.is_err(),
            "a personal workspace must not be assigned to two users"
        );
    }
}

