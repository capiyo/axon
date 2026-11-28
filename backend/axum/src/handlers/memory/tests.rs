#[cfg(test)]
mod session_context_tests {
    use super::*;
    use chrono::Utc;

    fn document(content: &str) -> MemoryDocument {
        let now = Utc::now();
        MemoryDocument {
            memory_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            project_id: None,
            task_id: None,
            owner_id: None,
            scope: "workspace_shared".into(),
            document_type: "fact".into(),
            version: 1,
            title: "Test memory".into(),
            content: content.into(),
            content_format: "text/plain".into(),
            tags: Vec::new(),
            source: "test".into(),
            source_kind: "test".into(),
            source_event_sequence: 1,
            created_at: now,
            updated_at: now,
            content_hash: String::new(),
            key_metadata: serde_json::Value::Null,
            value_payload: serde_json::Value::Null,
            qkv_version: "test-qkv-v1".into(),
            qoin_vector_version: None,
            status: "ready".into(),
            retention_policy: None,
        }
    }

    fn ranked_document(content: &str) -> RankedSessionMemoryDocument {
        RankedSessionMemoryDocument {
            document: document(content),
            qkv_attention_score: 0.5,
            qoin_score: ScoreBreakdown {
                semantic: 0.5,
                context: 0.5,
                dependency: 0.0,
                outcome: 0.5,
                failure: 0.5,
                evidence: 0.5,
                recency: 0.5,
                tags: 0.0,
                total: 0.5,
            },
            qoin_reasons: Vec::new(),
        }
    }

    #[tokio::test]
    async fn agent_session_access_is_bound_to_the_registering_user() {
        let state = AppState::default();
        let owner_id = Uuid::new_v4();
        let other_member_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        state.agents.write().await.insert(
            agent_id,
            crate::model::Agent {
                id: agent_id,
                workspace_id,
                created_by_user_id: Some(owner_id),
                display_name: "Owned agent".into(),
                provider: "manual".into(),
                connection_mode: crate::model::ConnectionMode::Manual,
                created_at: Utc::now(),
            },
        );

        assert!(
            require_agent_owner(&state, owner_id, workspace_id, agent_id)
                .await
                .is_ok()
        );
        assert!(
            require_agent_owner(&state, other_member_id, workspace_id, agent_id)
                .await
                .is_err()
        );
        assert!(
            require_agent_owner(&state, owner_id, Uuid::new_v4(), agent_id)
                .await
                .is_err()
        );
    }

    #[test]
    fn context_budget_omits_whole_ranked_documents_and_reports_truncation() {
        let first = ranked_document("short");
        let second = ranked_document(&"x".repeat(1024));
        let first_size = serde_json::to_vec(&first).unwrap().len();
        let (selected, used, truncated) =
            bounded_session_context(vec![first, second], first_size).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(used, first_size);
        assert!(truncated);
    }

    #[test]
    fn context_budget_preserves_all_ranked_documents_that_fit() {
        let first = ranked_document("first");
        let second = ranked_document("second");
        let budget =
            serde_json::to_vec(&first).unwrap().len() + serde_json::to_vec(&second).unwrap().len();
        let (selected, used, truncated) =
            bounded_session_context(vec![first, second], budget).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(used, budget);
        assert!(!truncated);
    }
}

