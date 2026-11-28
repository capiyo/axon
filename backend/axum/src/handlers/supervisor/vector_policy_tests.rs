#[cfg(test)]
mod vector_policy_tests {
    use super::*;

    #[test]
    fn production_and_credential_tasks_are_high_risk() {
        assert!(task_risk_score("Deploy", "Deploy to production") >= 0.80);
        assert!(task_risk_score("Auth", "Rotate the API credential") >= 0.80);
        assert!(task_risk_score("Shell", "Execute command on production host") >= 0.80);
        assert!(task_risk_score("Notes", "Summarize this paragraph") < 0.80);
    }

    #[test]
    fn vague_objectives_are_flagged_for_decomposition() {
        assert!(
            task_ambiguity("Build feature", "Figure it out somehow and handle the rest") >= 0.75
        );
        assert!(task_ambiguity("Summarize", "Summarize the supplied paragraph") < 0.75);
    }
}

