use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SupervisorError {
    #[error("checklist writers must be distinct and have non-empty identifiers")]
    InvalidWriters,
    #[error("checklist item key, summary and acceptance criteria must be non-empty")]
    InvalidChecklistItem,
    #[error("a checklist proposal contains a duplicate item key: {0}")]
    DuplicateChecklistKey(String),
    #[error("exactly two distinct supervisor reviews are required")]
    InvalidSupervisorCount,
    #[error("supervisor identifiers must be non-empty and distinct")]
    InvalidSupervisorIdentity,
    #[error("supervisor {supervisor_id} supplied no evidence")]
    MissingEvidence { supervisor_id: String },
    #[error("supervisor {supervisor_id} referenced unknown check {check_id}")]
    UnknownEvidenceCheck { supervisor_id: String, check_id: String },
    #[error("deterministic check identifiers must be non-empty and unique")]
    InvalidCheckSet,
    #[error("repair rounds used cannot exceed the configured maximum of two")]
    InvalidRepairRound,
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
            || item.acceptance_criteria.iter().any(|criterion| criterion.trim().is_empty())
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
    let keys: BTreeSet<_> = first_items.keys().chain(second_items.keys()).cloned().collect();

    let mut merged = Vec::with_capacity(keys.len());
    for key in keys {
        let left = first_items.get(&key);
        let right = second_items.get(&key);
        let mut criteria = Vec::new();
        let mut summaries = Vec::new();
        let mut proposed_by = Vec::new();

        if let Some(item) = left {
            summaries.push(item.summary.trim().to_owned());
            criteria.extend(item.acceptance_criteria.iter().map(|value| value.trim().to_owned()));
            proposed_by.push(first.writer_id.clone());
        }
        if let Some(item) = right {
            summaries.push(item.summary.trim().to_owned());
            criteria.extend(item.acceptance_criteria.iter().map(|value| value.trim().to_owned()));
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

/// Accept only when both independent supervisors approve with traceable evidence
/// and every deterministic check passes. Otherwise request repair, at most twice.
pub fn evaluate_supervision(
    checks: &[DeterministicCheck],
    reviews: &[SupervisorReview],
    repair_rounds_used: u8,
) -> Result<SupervisionDecision, SupervisorError> {
    if repair_rounds_used > MAX_REPAIR_ROUNDS {
        return Err(SupervisorError::InvalidRepairRound);
    }

    let mut check_ids = BTreeSet::new();
    if checks.iter().any(|check| check.id.trim().is_empty() || !check_ids.insert(&check.id)) {
        return Err(SupervisorError::InvalidCheckSet);
    }
    if reviews.len() != REQUIRED_SUPERVISOR_COUNT {
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
            id: "tests".into(),
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
