#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod scoring_config_tests {
    use super::*;

    #[test]
    fn default_scoring_policy_is_valid_and_reproducible() {
        let policy = QoinScoringConfig::default();
        assert!(policy.validate().is_ok());
        assert_eq!(policy.version, QOIN_MEMORY_SCORING_VERSION);
        assert!((policy.weights.total() - 1.08).abs() < 1e-12);
    }

    #[test]
    fn effective_policy_version_changes_when_any_scoring_parameter_changes() {
        let baseline = QoinScoringConfig::default();
        let same = QoinScoringConfig::default();
        assert_eq!(baseline.effective_version(), same.effective_version());

        let mut changed_weight = baseline.clone();
        changed_weight.weights.semantic += 0.01;
        assert_ne!(
            baseline.effective_version(),
            changed_weight.effective_version()
        );

        let mut changed_threshold = baseline.clone();
        changed_threshold.relevant_threshold += 0.01;
        assert_ne!(
            baseline.effective_version(),
            changed_threshold.effective_version()
        );

        let mut changed_feedback_weight = baseline.clone();
        changed_feedback_weight.feedback_weight = 0.75;
        assert_ne!(
            baseline.effective_version(),
            changed_feedback_weight.effective_version()
        );
    }

    #[test]
    fn legacy_scoring_config_defaults_feedback_weight_without_breaking_deserialization() {
        let mut value = serde_json::to_value(QoinScoringConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("feedback_weight");
        let parsed: QoinScoringConfig = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.feedback_weight, 0.25);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn invalid_weights_and_thresholds_are_rejected() {
        let mut policy = QoinScoringConfig::default();
        policy.weights.semantic = f64::NAN;
        assert_eq!(policy.validate(), Err(VectorError::InvalidScoringConfig));

        let mut policy = QoinScoringConfig::default();
        policy.conditional_threshold = 0.9;
        assert_eq!(policy.validate(), Err(VectorError::InvalidScoringConfig));

        let mut policy = QoinScoringConfig::default();
        policy.feedback_weight = 1.1;
        assert_eq!(policy.validate(), Err(VectorError::InvalidScoringConfig));
    }

    #[test]
    fn weights_are_normalized_by_their_sum() {
        let mut policy = QoinScoringConfig::default();
        policy.weights = QoinScoringWeights {
            semantic: 3.0,
            context: 1.5,
            dependency: 1.8,
            outcome: 1.0,
            failure: 1.2,
            evidence: 1.0,
            recency: 0.5,
            tags: 0.8,
        };
        assert!(policy.validate().is_ok());
        assert!((policy.weights.total() - 10.8).abs() < f64::EPSILON);
    }
}

