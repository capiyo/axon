#[cfg(test)]
mod search_budget_tests {
    use super::*;

    fn candidate(memory_id: Uuid, content: &str) -> RankedMemoryDocument {
        RankedMemoryDocument {
            memory_id,
            version: 1,
            title: "Budget test".into(),
            content: content.into(),
            content_hash: "hash".into(),
            source: "test".into(),
            source_kind: "test".into(),
            source_event_sequence: 1,
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

    #[test]
    fn search_budget_counts_full_serialized_results_and_reports_omissions() {
        let first = candidate(Uuid::new_v4(), "small");
        let second = candidate(Uuid::new_v4(), &"x".repeat(2048));
        let first_size = serde_json::to_vec(&first).unwrap().len();
        let (selected, used, truncated) =
            bounded_ranked_memory_documents(vec![first, second], first_size).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(used, first_size);
        assert!(truncated);
    }
}

