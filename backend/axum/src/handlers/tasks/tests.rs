#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_lifecycle_rejects_skipping_review() {
        assert!(!allowed_transition(
            &TaskStatus::Submitted,
            &TaskStatus::Completed
        ));
        assert!(!allowed_transition(
            &TaskStatus::Offered,
            &TaskStatus::InProgress
        ));
    }

    #[test]
    fn task_lifecycle_supports_review_and_revision() {
        assert!(allowed_transition(
            &TaskStatus::Submitted,
            &TaskStatus::ReviewRequired
        ));
        assert!(allowed_transition(
            &TaskStatus::ReviewRequired,
            &TaskStatus::ChangesRequested
        ));
        assert!(allowed_transition(
            &TaskStatus::ChangesRequested,
            &TaskStatus::InProgress
        ));
        assert!(allowed_transition(
            &TaskStatus::Accepted,
            &TaskStatus::Completed
        ));
    }
}

