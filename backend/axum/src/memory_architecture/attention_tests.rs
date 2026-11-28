#[cfg(test)]
mod tests {
    use super::*;
    fn q(values: Vec<f32>) -> QoinVector {
        QoinVector {
            values,
            model_version: "qoin-test-v1".into(),
        }
    }
    fn memory(id: Uuid, values: Vec<f32>, outcome: f32) -> AttentionMemory {
        AttentionMemory {
            memory_id: id,
            source_kind: "memory_entry".into(),
            key: q(values),
            value: format!("memory:{id}"),
            outcome_score: outcome,
            evidence_score: 0.9,
            source_event_sequence: 1,
            tags: BTreeSet::new(),
            dependency_ids: BTreeSet::new(),
        }
    }
    #[test]
    fn multi_head_attention_ranks_matching_keys() {
        let id = Uuid::new_v4();
        let query = AttentionQuery {
            query: q(vec![1.0, 0.0, 1.0, 0.0]),
            heads: 2,
            tags: BTreeSet::new(),
            dependency_ids: BTreeSet::new(),
        };
        let ranked = qkv_attention(
            &query,
            &[
                memory(Uuid::new_v4(), vec![0.0, 1.0, 0.0, 1.0], 1.0),
                memory(id, vec![1.0, 0.0, 1.0, 0.0], 0.8),
            ],
        )
        .unwrap();
        assert_eq!(ranked[0].memory_id, id);
    }
    #[test]
    fn attention_rejects_vectors_from_different_qoin_model_versions() {
        let mut historical = memory(Uuid::new_v4(), vec![1.0, 0.0, 1.0, 0.0], 0.8);
        historical.key.model_version = "edge-map-v0".into();
        let query = AttentionQuery {
            query: q(vec![1.0, 0.0, 1.0, 0.0]),
            heads: 2,
            tags: BTreeSet::new(),
            dependency_ids: BTreeSet::new(),
        };
        assert_eq!(
            qkv_attention(&query, &[historical]).unwrap_err(),
            AttentionError::InvalidVector
        );
    }

    #[test]
    fn pareto_selection_respects_budget_and_reports_real_coverage() {
        let scores = (0..10)
            .map(|i| AttentionScore {
                memory_id: Uuid::new_v4(),
                attention: 0.0,
                outcome: 0.0,
                evidence: 0.0,
                tag_overlap: 0.0,
                dependency_overlap: 0.0,
                total: if i == 0 { 9.0 } else { 1.0 },
                source_event_sequence: i,
            })
            .collect::<Vec<_>>();
        let selection = pareto_select(&scores, &ParetoPolicy::default()).unwrap();
        assert_eq!(selection.initial.len(), 2);
        assert!((selection.achieved_coverage - (10.0 / 18.0)).abs() < 1e-9);
        assert!(selection.budget_exhausted);
        assert_eq!(selection.deferred.len(), 8);
    }
}

