#[cfg(test)]
mod assignment_policy_tests {
    use super::*;

    #[test]
    fn exploration_is_limited_to_deterministically_low_risk_quick_tasks() {
        assert!(is_low_risk_quick(
            &TaskLevel::Quick,
            "Summarize",
            "Summarize this short note."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Normal,
            "Summarize",
            "Summarize this short note."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Quick,
            "Deploy",
            "Deploy this change to production."
        ));
        assert!(!is_low_risk_quick(
            &TaskLevel::Quick,
            "Review",
            &"x".repeat(4_001)
        ));
    }
}

