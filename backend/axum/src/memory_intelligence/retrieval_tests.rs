#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn vector(semantic: Vec<f32>) -> MemoryVector {
        MemoryVector {
            semantic,
            context: 0.9,
            outcome: 0.8,
            failure: 0.9,
            evidence: 0.95,
        }
    }
    fn query(workspace_id: Uuid, semantic: Vec<f32>) -> MemoryQuery {
        MemoryQuery {
            workspace_id,
            requester_agent_id: None,
            task_id: None,
            project_id: None,
            semantic,
            context: 0.9,
            outcome: 0.8,
            failure: 0.9,
            evidence: 0.9,
            now_unix: 2_000_000,
            tags: BTreeSet::new(),
            dependency_ids: BTreeSet::new(),
            outcome_labels: BTreeSet::new(),
            limit: 10,
        }
    }
    fn memory(workspace_id: Uuid, semantic: Vec<f32>, status: MemoryStatus) -> IndexedMemory {
        IndexedMemory {
            id: Uuid::new_v4(),
            workspace_id,
            chat_id: Uuid::new_v4(),
            task_id: None,
            project_id: None,
            owner_agent_id: None,
            visibility: MemoryVisibility::WorkspaceShared,
            status,
            vector: vector(semantic),
            tags: BTreeSet::new(),
            dependency_ids: BTreeSet::new(),
            outcome_labels: BTreeSet::new(),
            source_event_ids: vec![Uuid::new_v4()],
            created_at_unix: 1_999_000,
            superseded_by: None,
        }
    }

    #[test]
    fn retrieval_ranks_matching_memory_and_excludes_unapproved_or_foreign_records() {
        let workspace = Uuid::new_v4();
        let exact = memory(workspace, vec![1.0, 0.0], MemoryStatus::Approved);
        let exact_id = exact.id;
        let unrelated = memory(workspace, vec![0.0, 1.0], MemoryStatus::Approved);
        let proposed = memory(workspace, vec![1.0, 0.0], MemoryStatus::Proposed);
        let foreign = memory(Uuid::new_v4(), vec![1.0, 0.0], MemoryStatus::Approved);
        let result = retrieve_memories(
            &query(workspace, vec![1.0, 0.0]),
            &[unrelated, proposed, foreign, exact],
        )
        .unwrap();
        assert_eq!(result.ranked.first().unwrap().memory_id, exact_id);
        assert_eq!(result.excluded_by_scope_or_status, 2);
    }

    #[test]
    fn private_memory_is_visible_only_to_its_owner() {
        let workspace = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let mut item = memory(workspace, vec![1.0], MemoryStatus::Approved);
        item.visibility = MemoryVisibility::AgentPrivate;
        item.owner_agent_id = Some(owner);
        assert!(
            retrieve_memories(&query(workspace, vec![1.0]), &[item.clone()])
                .unwrap()
                .ranked
                .is_empty()
        );
        let mut authorized = query(workspace, vec![1.0]);
        authorized.requester_agent_id = Some(owner);
        assert_eq!(
            retrieve_memories(&authorized, &[item])
                .unwrap()
                .ranked
                .len(),
            1
        );
    }

    #[test]
    fn custom_scoring_weights_change_rank_score_and_report_policy_version() {
        let workspace = Uuid::new_v4();
        let mut item = memory(workspace, vec![1.0], MemoryStatus::Approved);
        item.vector.context = 0.0;
        let mut request = query(workspace, vec![1.0]);
        request.context = 1.0;

        let mut semantic_policy = QoinScoringConfig::default();
        semantic_policy.version = "semantic-only-test-v1".into();
        semantic_policy.weights = QoinScoringWeights {
            semantic: 1.0,
            context: 0.0,
            dependency: 0.0,
            outcome: 0.0,
            failure: 0.0,
            evidence: 0.0,
            recency: 0.0,
            tags: 0.0,
        };
        let mut context_policy = semantic_policy.clone();
        context_policy.version = "context-only-test-v1".into();
        context_policy.weights = QoinScoringWeights {
            semantic: 0.0,
            context: 1.0,
            dependency: 0.0,
            outcome: 0.0,
            failure: 0.0,
            evidence: 0.0,
            recency: 0.0,
            tags: 0.0,
        };

        let semantic_result =
            retrieve_memories_with_config(&request, &[item.clone()], &semantic_policy).unwrap();
        let context_result =
            retrieve_memories_with_config(&request, &[item], &context_policy).unwrap();

        assert_eq!(
            semantic_result.scoring_config.version,
            "semantic-only-test-v1"
        );
        assert_eq!(
            context_result.scoring_config.version,
            "context-only-test-v1"
        );
        assert!(semantic_result.ranked[0].score.total > context_result.ranked[0].score.total);
    }

    #[test]
    fn vector_validation_rejects_invalid_values_and_dimensions() {
        let workspace = Uuid::new_v4();
        let mut item = memory(workspace, vec![1.0, 0.0], MemoryStatus::Approved);
        item.vector.context = f32::NAN;
        assert_eq!(
            retrieve_memories(&query(workspace, vec![1.0, 0.0]), &[item]).unwrap_err(),
            VectorError::DimensionOutOfRange
        );
        let item = memory(workspace, vec![1.0, 0.0], MemoryStatus::Approved);
        assert_eq!(
            retrieve_memories(&query(workspace, vec![1.0]), &[item]).unwrap_err(),
            VectorError::DimensionMismatch
        );
    }

    #[test]
    fn replay_reconstructs_and_compares_full_historical_state() {
        let id = Uuid::new_v4();
        let proposed = HistoricalState {
            text: "retain immutable history".into(),
            status: MemoryStatus::Proposed,
        };
        let approved = HistoricalState {
            text: "retain immutable history".into(),
            status: MemoryStatus::Approved,
        };
        let events = vec![
            HistoricalEvent {
                sequence: 1,
                event_id: Uuid::new_v4(),
                memory_id: id,
                action: HistoricalAction::Created,
                snapshot: proposed,
            },
            HistoricalEvent {
                sequence: 2,
                event_id: Uuid::new_v4(),
                memory_id: id,
                action: HistoricalAction::Approved,
                snapshot: approved.clone(),
            },
        ];
        let expected = BTreeMap::from([(id, approved)]);
        let report = reconstruct_history(&events, Some(&expected));
        assert!(report.issues.is_empty());
        assert_eq!(report.matches_expected, Some(true));
    }

    #[test]
    fn replay_reports_invalid_transition_and_expected_state_mismatch() {
        let id = Uuid::new_v4();
        let state = HistoricalState {
            text: "invalid".into(),
            status: MemoryStatus::Approved,
        };
        let events = vec![HistoricalEvent {
            sequence: 1,
            event_id: Uuid::new_v4(),
            memory_id: id,
            action: HistoricalAction::Approved,
            snapshot: state,
        }];
        let expected = BTreeMap::from([(
            id,
            HistoricalState {
                text: "expected".into(),
                status: MemoryStatus::Proposed,
            },
        )]);
        let report = reconstruct_history(&events, Some(&expected));
        assert!(!report.issues.is_empty());
        assert_eq!(report.matches_expected, Some(false));
    }
}

