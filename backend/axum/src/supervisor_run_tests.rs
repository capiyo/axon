use crate::{
    audit::AuditEvent,
    storage::{SqliteStorage, StoredEntity},
    supervisor::{
    ChecklistItem, ChecklistProposal, DeterministicCheck, EvidenceReference, SupervisorReview,
    SupervisorRun, SupervisorRunStatus, SupervisorVerdict, SupervisionOutcome, WorkerResult,
    },
    AppState,
};
use uuid::Uuid;

fn proposal(writer_id: &str) -> ChecklistProposal {
    ChecklistProposal {
        writer_id: writer_id.into(),
        items: vec![ChecklistItem {
            key: "build".into(),
            summary: "Build the application".into(),
            acceptance_criteria: vec!["The production build passes".into()],
            required: true,
        }],
    }
}

fn worker(worker_id: &str, artifact_id: &str) -> WorkerResult {
    WorkerResult {
        worker_id: worker_id.into(),
        summary: format!("{worker_id} completed its assigned work."),
        artifact_ids: vec![artifact_id.into()],
    }
}

fn review(supervisor_id: &str, verdict: SupervisorVerdict) -> SupervisorReview {
    SupervisorReview {
        supervisor_id: supervisor_id.into(),
        verdict,
        evidence: vec![EvidenceReference {
            check_id: "build".into(),
            source: "ci://run/1".into(),
            observation: "The CI build completed and its result is attached.".into(),
        }],
    }
}

fn checks(passed: bool) -> Vec<DeterministicCheck> {
    vec![DeterministicCheck {
        id: "build".into(),
        passed,
        evidence_uri: Some("ci://run/1".into()),
    }]
}

#[test]
fn supervisor_run_advances_only_after_each_independent_stage() {
    let mut run = SupervisorRun::new(
        Uuid::new_v4(),
        "Implement supervisor".into(),
        "Complete the requested implementation with evidence.".into(),
    )
    .unwrap();

    run.submit_checklist(proposal("writer-a")).unwrap();
    assert_eq!(run.status, SupervisorRunStatus::CollectingChecklists);
    run.submit_checklist(proposal("writer-b")).unwrap();
    assert_eq!(run.status, SupervisorRunStatus::ReadyForWorkers);

    run.submit_worker_result(worker("worker-a", "artifact-a"))
        .unwrap();
    assert_eq!(run.status, SupervisorRunStatus::WorkersRunning);
    run.submit_worker_result(worker("worker-b", "artifact-b"))
        .unwrap();
    assert_eq!(run.status, SupervisorRunStatus::AwaitingChecks);

    run.record_checks(checks(true)).unwrap();
    assert_eq!(run.status, SupervisorRunStatus::AwaitingReviews);
    assert!(run
        .submit_review(review("supervisor-a", SupervisorVerdict::Approve))
        .unwrap()
        .is_none());
    let decision = run
        .submit_review(review("supervisor-b", SupervisorVerdict::Approve))
        .unwrap()
        .unwrap();

    assert_eq!(decision.outcome, SupervisionOutcome::Accepted);
    assert_eq!(run.status, SupervisorRunStatus::Accepted);
}

#[test]
fn repair_rounds_are_bounded_and_terminal_failure_cannot_restart() {
    let mut run = SupervisorRun::new(
        Uuid::new_v4(),
        "Repair test".into(),
        "Exercise the two repair limit.".into(),
    )
    .unwrap();
    run.submit_checklist(proposal("writer-a")).unwrap();
    run.submit_checklist(proposal("writer-b")).unwrap();

    for round in 0..=2 {
        run.submit_worker_result(worker("worker-a", "artifact-a"))
            .unwrap();
        run.submit_worker_result(worker("worker-b", "artifact-b"))
            .unwrap();
        run.record_checks(checks(false)).unwrap();
        run.submit_review(review("supervisor-a", SupervisorVerdict::Approve))
            .unwrap();
        let decision = run
            .submit_review(review("supervisor-b", SupervisorVerdict::Approve))
            .unwrap()
            .unwrap();

        if round < 2 {
            assert_eq!(decision.outcome, SupervisionOutcome::RepairRequired);
            run.start_repair().unwrap();
            assert_eq!(run.repair_rounds_used, round + 1);
        } else {
            assert_eq!(decision.outcome, SupervisionOutcome::Failed);
            assert_eq!(run.status, SupervisorRunStatus::Failed);
            assert!(run.start_repair().is_err());
        }
    }
}

#[tokio::test]
async fn supervisor_run_is_restored_from_sqlite_after_restart() {
    let storage = SqliteStorage::connect_in_memory().await.unwrap().shared();
    let state = AppState::with_storage(storage.clone()).await.unwrap();
    let workspace = state
        .ensure_firebase_user("axon-supervisor-test", "supervisor-user")
        .await
        .unwrap()
        .workspace;
    state
        .workspaces
        .write()
        .await
        .insert(workspace.id, workspace.clone());

    let run = SupervisorRun::new(
        workspace.id,
        "Persistent supervisor run".into(),
        "Verify supervisor state survives process reconstruction.".into(),
    )
    .unwrap();
    state
        .persist_entity(
            StoredEntity::SupervisorRun(run.clone()),
            Vec::new(),
            AuditEvent::attributed(
                "human:test",
                Some(workspace.id),
                "supervisor_run.created",
                "supervisor_run",
                run.id.to_string(),
                serde_json::json!({"status": "collecting_checklists"}),
            ),
        )
        .await
        .unwrap();

    let restored = AppState::with_storage(storage).await.unwrap();
    assert_eq!(
        restored.supervisor_runs.read().await.get(&run.id),
        Some(&run)
    );
}
