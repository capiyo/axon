use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLevel { Quick, Normal, Important }
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const REQUIRED_WORKER_COUNT: usize = 2;
pub const REQUIRED_SUPERVISOR_COUNT: usize = 2;
pub const MAX_REPAIR_ROUNDS: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistItem {
    pub key: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistProposal {
    pub writer_id: String,
    pub items: Vec<ChecklistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChecklistPriority {
    Required,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergedChecklistItem {
    pub key: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: ChecklistPriority,
    pub proposed_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerResult {
    pub worker_id: String,
    pub summary: String,
    pub artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterministicCheck {
    pub id: String,
    pub passed: bool,
    pub evidence_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceReference {
    pub check_id: String,
    pub source: String,
    pub observation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorVerdict {
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorReview {
    pub supervisor_id: String,
    pub verdict: SupervisorVerdict,
    pub evidence: Vec<EvidenceReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisionOutcome {
    Accepted,
    RepairRequired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisionDecision {
    pub outcome: SupervisionOutcome,
    pub repair_rounds_used: u8,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorRunStatus {
    CollectingChecklists,
    ReadyForWorkers,
    WorkersRunning,
    AwaitingChecks,
    AwaitingReviews,
    RepairRequired,
    Accepted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorRun {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub objective: String,
    pub status: SupervisorRunStatus,
    pub version: u64,
    pub repair_rounds_used: u8,
    pub checklist_proposals: Vec<ChecklistProposal>,
    pub merged_checklist: Vec<MergedChecklistItem>,
    pub worker_results: Vec<WorkerResult>,
    pub deterministic_checks: Vec<DeterministicCheck>,
    pub reviews: Vec<SupervisorReview>,
    pub decision: Option<SupervisionDecision>,
    #[serde(default)]
    pub assignment_level: Option<TaskLevel>,
    #[serde(default)]
    pub assignment_category: Option<String>,
    #[serde(default)]
    pub assignment_notice: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SupervisorError {
    #[error("checklist writers must be distinct and have non-empty identifiers")]
    InvalidWriters,
    #[error("checklist item key, summary and acceptance criteria must be non-empty")]
    InvalidChecklistItem,
    #[error("a checklist proposal contains a duplicate item key: {0}")]
    DuplicateChecklistKey(String),
    #[error("the configured number of independent worker results is required")]
    InvalidWorkerCount,
    #[error("worker identifiers must be non-empty and distinct")]
    InvalidWorkerIdentity,
    #[error("each worker result must include a summary and at least one artifact")]
    MissingWorkerEvidence,
    #[error("the configured number of distinct supervisor reviews is required")]
    InvalidSupervisorCount,
    #[error("supervisor identifiers must be non-empty and distinct")]
    InvalidSupervisorIdentity,
    #[error("supervisor {supervisor_id} supplied no evidence")]
    MissingEvidence { supervisor_id: String },
    #[error("supervisor {supervisor_id} referenced unknown check {check_id}")]
    UnknownEvidenceCheck {
        supervisor_id: String,
        check_id: String,
    },
    #[error("deterministic check identifiers must be non-empty and unique")]
    InvalidCheckSet,
    #[error("repair rounds used cannot exceed the configured maximum of two")]
    InvalidRepairRound,
    #[error("supervisor run title and objective must be non-empty")]
    InvalidRunInput,
    #[error("operation is not valid in the current supervisor run state")]
    InvalidRunState,
    #[error("this role has already submitted to the current run stage")]
    DuplicateSubmission,
    #[error("a repair is not available after the configured repair limit")]
    RepairNotAvailable,
}

fn required_role_counts(level: Option<&TaskLevel>) -> (usize, usize, usize) {
    match level {
        Some(TaskLevel::Quick) => (0, 1, 0),
        Some(TaskLevel::Normal) => (1, 1, 1),
        Some(TaskLevel::Important) | None => (2, 2, 2),
    }
}

impl SupervisorRun {
    pub fn new(
        workspace_id: Uuid,
        title: String,
        objective: String,
    ) -> Result<Self, SupervisorError> {
        let title = title.trim();
        let objective = objective.trim();
        if title.is_empty() || objective.is_empty() {
            return Err(SupervisorError::InvalidRunInput);
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            workspace_id,
            title: title.to_owned(),
            objective: objective.to_owned(),
            status: SupervisorRunStatus::CollectingChecklists,
            version: 0,
            repair_rounds_used: 0,
            checklist_proposals: Vec::new(),
            merged_checklist: Vec::new(),
            worker_results: Vec::new(),
            deterministic_checks: Vec::new(),
            reviews: Vec::new(),
            decision: None,
            assignment_level: None,
            assignment_category: None,
            assignment_notice: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn submit_checklist(&mut self, proposal: ChecklistProposal) -> Result<(), SupervisorError> {
        if self.status != SupervisorRunStatus::CollectingChecklists {
            return Err(SupervisorError::InvalidRunState);
        }
        if proposal.writer_id.trim().is_empty()
            || self
                .checklist_proposals
                .iter()
                .any(|item| item.writer_id.trim() == proposal.writer_id.trim())
        {
            return Err(SupervisorError::DuplicateSubmission);
        }
        index_proposal(&proposal)?;
        let required_writers = required_role_counts(self.assignment_level.as_ref()).0;
        if required_writers == 0 {
            return Err(SupervisorError::InvalidRunState);
        }
        let mut proposals = self.checklist_proposals.clone();
        proposals.push(proposal);
        if proposals.len() > required_writers {
            return Err(SupervisorError::DuplicateSubmission);
        }
        let merged = if proposals.len() == required_writers {
            if required_writers == 1 {
                let indexed = index_proposal(&proposals[0])?;
                Some(
                    indexed
                        .into_iter()
                        .map(|(key, item)| MergedChecklistItem {
                            key,
                            summary: item.summary,
                            acceptance_criteria: item.acceptance_criteria,
                            priority: if item.required {
                                ChecklistPriority::Required
                            } else {
                                ChecklistPriority::Optional
                            },
                            proposed_by: vec![proposals[0].writer_id.clone()],
                        })
                        .collect(),
                )
            } else {
                Some(merge_checklists(&proposals[0], &proposals[1])?)
            }
        } else {
            None
        };
        self.checklist_proposals = proposals;
        if let Some(merged) = merged {
            self.merged_checklist = merged;
            self.status = SupervisorRunStatus::ReadyForWorkers;
        }
        self.touch();
        Ok(())
    }

    pub fn submit_worker_result(&mut self, result: WorkerResult) -> Result<(), SupervisorError> {
        if !matches!(
            self.status,
            SupervisorRunStatus::ReadyForWorkers | SupervisorRunStatus::WorkersRunning
        ) {
            return Err(SupervisorError::InvalidRunState);
        }
        if result.worker_id.trim().is_empty()
            || self
                .worker_results
                .iter()
                .any(|item| item.worker_id.trim() == result.worker_id.trim())
        {
            return Err(SupervisorError::DuplicateSubmission);
        }
        let required_workers = required_role_counts(self.assignment_level.as_ref()).1;
        let mut results = self.worker_results.clone();
        results.push(result);
        if results.len() > required_workers {
            return Err(SupervisorError::InvalidWorkerCount);
        }
        if results.len() == required_workers {
            validate_worker_results_with_count(&results, required_workers)?;
        }
        self.worker_results = results;
        self.status = if self.worker_results.len() == required_workers {
            SupervisorRunStatus::AwaitingChecks
        } else {
            SupervisorRunStatus::WorkersRunning
        };
        self.touch();
        Ok(())
    }

    pub fn record_checks(
        &mut self,
        checks: Vec<DeterministicCheck>,
    ) -> Result<(), SupervisorError> {
        if self.status != SupervisorRunStatus::AwaitingChecks || checks.is_empty() {
            return Err(SupervisorError::InvalidRunState);
        }
        let mut check_ids = BTreeSet::new();
        if checks
            .iter()
            .any(|check| check.id.trim().is_empty() || !check_ids.insert(&check.id))
        {
            return Err(SupervisorError::InvalidCheckSet);
        }
        self.deterministic_checks = checks;
        if required_role_counts(self.assignment_level.as_ref()).2 == 0 {
            let failed: Vec<_> = self
                .deterministic_checks
                .iter()
                .filter(|check| !check.passed)
                .map(|check| format!("Deterministic check failed: {}", check.id))
                .collect();
            let outcome = if failed.is_empty() {
                SupervisionOutcome::Accepted
            } else if self.repair_rounds_used < MAX_REPAIR_ROUNDS {
                SupervisionOutcome::RepairRequired
            } else {
                SupervisionOutcome::Failed
            };
            self.status = match outcome {
                SupervisionOutcome::Accepted => SupervisorRunStatus::Accepted,
                SupervisionOutcome::RepairRequired => SupervisorRunStatus::RepairRequired,
                SupervisionOutcome::Failed => SupervisorRunStatus::Failed,
            };
            self.decision = Some(SupervisionDecision {
                outcome,
                repair_rounds_used: self.repair_rounds_used,
                reasons: if failed.is_empty() {
                    vec!["Quick task passed all deterministic checks.".into()]
                } else {
                    failed
                },
            });
        } else {
            self.status = SupervisorRunStatus::AwaitingReviews;
        }
        self.touch();
        Ok(())
    }

    pub fn submit_review(
        &mut self,
        review: SupervisorReview,
    ) -> Result<Option<SupervisionDecision>, SupervisorError> {
        if self.status != SupervisorRunStatus::AwaitingReviews {
            return Err(SupervisorError::InvalidRunState);
        }
        if review.supervisor_id.trim().is_empty()
            || self
                .reviews
                .iter()
                .any(|item| item.supervisor_id.trim() == review.supervisor_id.trim())
        {
            return Err(SupervisorError::DuplicateSubmission);
        }
        if review.evidence.is_empty()
            || review.evidence.iter().any(|evidence| {
                evidence.source.trim().is_empty()
                    || evidence.observation.trim().is_empty()
                    || !self
                        .deterministic_checks
                        .iter()
                        .any(|check| check.id == evidence.check_id)
            })
        {
            return Err(SupervisorError::MissingEvidence {
                supervisor_id: review.supervisor_id,
            });
        }

        let required_supervisors = required_role_counts(self.assignment_level.as_ref()).2;
        if required_supervisors == 0 {
            return Err(SupervisorError::InvalidRunState);
        }
        let mut reviews = self.reviews.clone();
        reviews.push(review);
        if reviews.len() < required_supervisors {
            self.reviews = reviews;
            self.touch();
            return Ok(None);
        }
        let decision = evaluate_supervision_with_count(
            &self.deterministic_checks,
            &reviews,
            self.repair_rounds_used,
            required_supervisors,
        )?;
        self.reviews = reviews;
        self.status = match decision.outcome {
            SupervisionOutcome::Accepted => SupervisorRunStatus::Accepted,
            SupervisionOutcome::RepairRequired => SupervisorRunStatus::RepairRequired,
            SupervisionOutcome::Failed => SupervisorRunStatus::Failed,
        };
        self.decision = Some(decision.clone());
        self.touch();
        Ok(Some(decision))
    }

    pub fn start_repair(&mut self) -> Result<(), SupervisorError> {
        if self.status != SupervisorRunStatus::RepairRequired
            || self.repair_rounds_used >= MAX_REPAIR_ROUNDS
        {
            return Err(SupervisorError::RepairNotAvailable);
        }
        self.repair_rounds_used += 1;
        self.worker_results.clear();
        self.deterministic_checks.clear();
        self.reviews.clear();
        self.decision = None;
        self.status = SupervisorRunStatus::ReadyForWorkers;
        self.touch();
        Ok(())
    }

    fn touch(&mut self) {
        self.version += 1;
        self.updated_at = Utc::now();
    }
}

fn normalized_key(key: &str) -> Result<String, SupervisorError> {
    let normalized = key.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(SupervisorError::InvalidChecklistItem);
    }
    Ok(normalized)
}

fn index_proposal(
    proposal: &ChecklistProposal,
) -> Result<BTreeMap<String, ChecklistItem>, SupervisorError> {
    let mut indexed = BTreeMap::new();
    for item in &proposal.items {
        let key = normalized_key(&item.key)?;
        if item.summary.trim().is_empty()
            || item.acceptance_criteria.is_empty()
            || item
                .acceptance_criteria
                .iter()
                .any(|criterion| criterion.trim().is_empty())
        {
            return Err(SupervisorError::InvalidChecklistItem);
        }
        if indexed.insert(key.clone(), item.clone()).is_some() {
            return Err(SupervisorError::DuplicateChecklistKey(key));
        }
    }
    Ok(indexed)
}

/// Merge two independently authored checklists without relying on proposal order.
/// An item is REQUIRED only when both writers independently mark the same key required.
pub fn merge_checklists(
    first: &ChecklistProposal,
    second: &ChecklistProposal,
) -> Result<Vec<MergedChecklistItem>, SupervisorError> {
    if first.writer_id.trim().is_empty()
        || second.writer_id.trim().is_empty()
        || first.writer_id == second.writer_id
    {
        return Err(SupervisorError::InvalidWriters);
    }

    let first_items = index_proposal(first)?;
    let second_items = index_proposal(second)?;
    let keys: BTreeSet<_> = first_items
        .keys()
        .chain(second_items.keys())
        .cloned()
        .collect();

    let mut merged = Vec::with_capacity(keys.len());
    for key in keys {
        let left = first_items.get(&key);
        let right = second_items.get(&key);
        let mut criteria = Vec::new();
        let mut summaries = Vec::new();
        let mut proposed_by = Vec::new();

        if let Some(item) = left {
            summaries.push(item.summary.trim().to_owned());
            criteria.extend(
                item.acceptance_criteria
                    .iter()
                    .map(|value| value.trim().to_owned()),
            );
            proposed_by.push(first.writer_id.clone());
        }
        if let Some(item) = right {
            summaries.push(item.summary.trim().to_owned());
            criteria.extend(
                item.acceptance_criteria
                    .iter()
                    .map(|value| value.trim().to_owned()),
            );
            proposed_by.push(second.writer_id.clone());
        }

        summaries.sort();
        summaries.dedup();
        criteria.sort();
        criteria.dedup();
        proposed_by.sort();

        merged.push(MergedChecklistItem {
            key,
            summary: summaries.join(" / "),
            acceptance_criteria: criteria,
            priority: if left.is_some_and(|item| item.required)
                && right.is_some_and(|item| item.required)
            {
                ChecklistPriority::Required
            } else {
                ChecklistPriority::Optional
            },
            proposed_by,
        });
    }
    Ok(merged)
}

/// Validate the two independent worker outputs before deterministic checks begin.
pub fn validate_worker_results(results: &[WorkerResult]) -> Result<(), SupervisorError> {
    validate_worker_results_with_count(results, REQUIRED_WORKER_COUNT)
}

fn validate_worker_results_with_count(
    results: &[WorkerResult],
    required_count: usize,
) -> Result<(), SupervisorError> {
    if results.len() != required_count {
        return Err(SupervisorError::InvalidWorkerCount);
    }

    let mut worker_ids = BTreeSet::new();
    for result in results {
        if result.worker_id.trim().is_empty() || !worker_ids.insert(result.worker_id.trim()) {
            return Err(SupervisorError::InvalidWorkerIdentity);
        }
        if result.summary.trim().is_empty()
            || result.artifact_ids.is_empty()
            || result.artifact_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err(SupervisorError::MissingWorkerEvidence);
        }
    }
    Ok(())
}

/// Accept only when both independent supervisors approve with traceable evidence
/// and every deterministic check passes. Otherwise request repair, at most twice.
pub fn evaluate_supervision(
    checks: &[DeterministicCheck],
    reviews: &[SupervisorReview],
    repair_rounds_used: u8,
) -> Result<SupervisionDecision, SupervisorError> {
    evaluate_supervision_with_count(
        checks,
        reviews,
        repair_rounds_used,
        REQUIRED_SUPERVISOR_COUNT,
    )
}

fn evaluate_supervision_with_count(
    checks: &[DeterministicCheck],
    reviews: &[SupervisorReview],
    repair_rounds_used: u8,
    required_supervisors: usize,
) -> Result<SupervisionDecision, SupervisorError> {
    if repair_rounds_used > MAX_REPAIR_ROUNDS {
        return Err(SupervisorError::InvalidRepairRound);
    }

    let mut check_ids = BTreeSet::new();
    if checks
        .iter()
        .any(|check| check.id.trim().is_empty() || !check_ids.insert(&check.id))
    {
        return Err(SupervisorError::InvalidCheckSet);
    }
    if reviews.len() != required_supervisors {
        return Err(SupervisorError::InvalidSupervisorCount);
    }

    let mut supervisor_ids = BTreeSet::new();
    for review in reviews {
        if review.supervisor_id.trim().is_empty()
            || !supervisor_ids.insert(review.supervisor_id.clone())
        {
            return Err(SupervisorError::InvalidSupervisorIdentity);
        }
        if review.evidence.is_empty() {
            return Err(SupervisorError::MissingEvidence {
                supervisor_id: review.supervisor_id.clone(),
            });
        }
        for evidence in &review.evidence {
            if evidence.source.trim().is_empty() || evidence.observation.trim().is_empty() {
                return Err(SupervisorError::MissingEvidence {
                    supervisor_id: review.supervisor_id.clone(),
                });
            }
            if !check_ids.contains(&evidence.check_id) {
                return Err(SupervisorError::UnknownEvidenceCheck {
                    supervisor_id: review.supervisor_id.clone(),
                    check_id: evidence.check_id.clone(),
                });
            }
        }
    }

    let failed_checks: Vec<_> = checks.iter().filter(|check| !check.passed).collect();
    let dissenting: Vec<_> = reviews
        .iter()
        .filter(|review| review.verdict != SupervisorVerdict::Approve)
        .map(|review| format!("{} requested changes", review.supervisor_id))
        .collect();

    if failed_checks.is_empty() && dissenting.is_empty() {
        return Ok(SupervisionDecision {
            outcome: SupervisionOutcome::Accepted,
            repair_rounds_used,
            reasons: vec!["Both supervisors approved and all deterministic checks passed.".into()],
        });
    }

    let mut reasons = dissenting;
    reasons.extend(
        failed_checks
            .iter()
            .map(|check| format!("Deterministic check failed: {}", check.id)),
    );
    let outcome = if repair_rounds_used < MAX_REPAIR_ROUNDS {
        SupervisionOutcome::RepairRequired
    } else {
        SupervisionOutcome::Failed
    };
    Ok(SupervisionDecision {
        outcome,
        repair_rounds_used,
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, required: bool) -> ChecklistItem {
        ChecklistItem {
            key: key.into(),
            summary: format!("Complete {key}"),
            acceptance_criteria: vec![format!("{key} is verified")],
            required,
        }
    }

    fn review(id: &str, verdict: SupervisorVerdict) -> SupervisorReview {
        SupervisorReview {
            supervisor_id: id.into(),
            verdict,
            evidence: vec![EvidenceReference {
                check_id: "build".into(),
                source: "ci://run/123".into(),
                observation: "The production build completed.".into(),
            }],
        }
    }

    #[test]
    fn normal_level_uses_one_writer_one_worker_and_one_supervisor() {
        let mut run =
            SupervisorRun::new(Uuid::new_v4(), "Normal".into(), "Complete task".into()).unwrap();
        run.assignment_level = Some(TaskLevel::Normal);
        run.submit_checklist(ChecklistProposal {
            writer_id: "writer-a".into(),
            items: vec![item("build", true)],
        })
        .unwrap();
        assert_eq!(run.status, SupervisorRunStatus::ReadyForWorkers);
        assert_eq!(run.merged_checklist.len(), 1);
        run.submit_worker_result(WorkerResult {
            worker_id: "worker-a".into(),
            summary: "Done".into(),
            artifact_ids: vec!["artifact-1".into()],
        })
        .unwrap();
        assert_eq!(run.status, SupervisorRunStatus::AwaitingChecks);
        run.record_checks(vec![DeterministicCheck {
            id: "build".into(),
            passed: true,
            evidence_uri: Some("ci://build".into()),
        }])
        .unwrap();
        assert_eq!(run.status, SupervisorRunStatus::AwaitingReviews);
        let decision = run
            .submit_review(review("supervisor-a", SupervisorVerdict::Approve))
            .unwrap()
            .unwrap();
        assert_eq!(decision.outcome, SupervisionOutcome::Accepted);
    }

    #[test]
    fn quick_level_skips_checklist_and_supervisor_but_requires_deterministic_checks() {
        let mut run =
            SupervisorRun::new(Uuid::new_v4(), "Quick".into(), "Simple task".into()).unwrap();
        run.assignment_level = Some(TaskLevel::Quick);
        run.status = SupervisorRunStatus::ReadyForWorkers;
        run.submit_worker_result(WorkerResult {
            worker_id: "worker-a".into(),
            summary: "Done".into(),
            artifact_ids: vec!["artifact-1".into()],
        })
        .unwrap();
        run.record_checks(vec![DeterministicCheck {
            id: "format".into(),
            passed: true,
            evidence_uri: Some("check://format".into()),
        }])
        .unwrap();
        assert_eq!(run.status, SupervisorRunStatus::Accepted);
        assert_eq!(run.decision.unwrap().outcome, SupervisionOutcome::Accepted);
    }

    #[test]
    fn quick_level_requests_repair_when_a_deterministic_check_fails() {
        let mut run =
            SupervisorRun::new(Uuid::new_v4(), "Quick".into(), "Simple task".into()).unwrap();
        run.assignment_level = Some(TaskLevel::Quick);
        run.status = SupervisorRunStatus::ReadyForWorkers;
        run.submit_worker_result(WorkerResult {
            worker_id: "worker-a".into(),
            summary: "Done".into(),
            artifact_ids: vec!["artifact-1".into()],
        })
        .unwrap();
        run.record_checks(vec![DeterministicCheck {
            id: "format".into(),
            passed: false,
            evidence_uri: None,
        }])
        .unwrap();
        assert_eq!(run.status, SupervisorRunStatus::RepairRequired);
    }

    #[test]
    fn checklist_requires_independent_agreement() {
        let first = ChecklistProposal {
            writer_id: "writer-a".into(),
            items: vec![item("auth", true), item("docs", true)],
        };
        let second = ChecklistProposal {
            writer_id: "writer-b".into(),
            items: vec![item("auth", true), item("docs", false), item("tests", true)],
        };
        let merged = merge_checklists(&first, &second).unwrap();

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].key, "auth");
        assert_eq!(merged[0].priority, ChecklistPriority::Required);
        assert_eq!(merged[1].key, "docs");
        assert_eq!(merged[1].priority, ChecklistPriority::Optional);
        assert_eq!(merged[2].key, "tests");
        assert_eq!(merged[2].priority, ChecklistPriority::Optional);
    }

    #[test]
    fn checklist_merge_is_independent_of_writer_order() {
        let first = ChecklistProposal {
            writer_id: "writer-a".into(),
            items: vec![item("alpha", true)],
        };
        let second = ChecklistProposal {
            writer_id: "writer-b".into(),
            items: vec![item("beta", true)],
        };
        assert_eq!(
            merge_checklists(&first, &second).unwrap(),
            merge_checklists(&second, &first).unwrap()
        );
    }

    #[test]
    fn duplicate_keys_in_one_proposal_are_rejected() {
        let proposal = ChecklistProposal {
            writer_id: "writer-a".into(),
            items: vec![item("same", true), item("same", false)],
        };
        let other = ChecklistProposal {
            writer_id: "writer-b".into(),
            items: vec![],
        };
        assert_eq!(
            merge_checklists(&proposal, &other),
            Err(SupervisorError::DuplicateChecklistKey("same".into()))
        );
    }

    #[test]
    fn exactly_two_independent_workers_must_return_artifacts() {
        let results = vec![
            WorkerResult {
                worker_id: "worker-a".into(),
                summary: "Backend change.".into(),
                artifact_ids: vec!["artifact-a".into()],
            },
            WorkerResult {
                worker_id: "worker-b".into(),
                summary: "Frontend change.".into(),
                artifact_ids: vec!["artifact-b".into()],
            },
        ];
        assert_eq!(validate_worker_results(&results), Ok(()));
        assert_eq!(
            validate_worker_results(&results[..1]),
            Err(SupervisorError::InvalidWorkerCount)
        );
    }

    #[test]
    fn duplicate_worker_identity_is_rejected() {
        let results = vec![
            WorkerResult {
                worker_id: "worker-a".into(),
                summary: "First.".into(),
                artifact_ids: vec!["artifact-a".into()],
            },
            WorkerResult {
                worker_id: "worker-a".into(),
                summary: "Second.".into(),
                artifact_ids: vec!["artifact-b".into()],
            },
        ];
        assert_eq!(
            validate_worker_results(&results),
            Err(SupervisorError::InvalidWorkerIdentity)
        );
    }

    #[test]
    fn two_approvals_and_passing_checks_accept() {
        let checks = vec![DeterministicCheck {
            id: "build".into(),
            passed: true,
            evidence_uri: Some("ci://run/123".into()),
        }];
        let reviews = vec![
            review("supervisor-a", SupervisorVerdict::Approve),
            review("supervisor-b", SupervisorVerdict::Approve),
        ];
        let decision = evaluate_supervision(&checks, &reviews, 0).unwrap();
        assert_eq!(decision.outcome, SupervisionOutcome::Accepted);
    }

    #[test]
    fn failed_check_requests_two_repairs_then_fails_closed() {
        let checks = vec![DeterministicCheck {
            id: "build".into(),
            passed: false,
            evidence_uri: Some("ci://run/456".into()),
        }];
        let reviews = vec![
            review("supervisor-a", SupervisorVerdict::Approve),
            review("supervisor-b", SupervisorVerdict::Approve),
        ];
        assert_eq!(
            evaluate_supervision(&checks, &reviews, 0).unwrap().outcome,
            SupervisionOutcome::RepairRequired
        );
        assert_eq!(
            evaluate_supervision(&checks, &reviews, 1).unwrap().outcome,
            SupervisionOutcome::RepairRequired
        );
        assert_eq!(
            evaluate_supervision(&checks, &reviews, 2).unwrap().outcome,
            SupervisionOutcome::Failed
        );
    }

    #[test]
    fn review_without_evidence_is_rejected() {
        let checks = vec![DeterministicCheck {
            id: "build".into(),
            passed: true,
            evidence_uri: None,
        }];
        let mut invalid = review("supervisor-a", SupervisorVerdict::Approve);
        invalid.evidence.clear();
        assert_eq!(
            evaluate_supervision(
                &checks,
                &[invalid, review("supervisor-b", SupervisorVerdict::Approve)],
                0
            ),
            Err(SupervisorError::MissingEvidence {
                supervisor_id: "supervisor-a".into()
            })
        );
    }
}
