//! Deterministic vector retrieval and historical reconstruction primitives.
//!
//! The immutable event log remains the source of truth. Vectors are a
//! searchable index, never the only surviving representation of a memory.
//! Encryption belongs at the storage/KMS boundary; no custom cipher is used.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const RELEVANCE_THRESHOLD: f64 = 0.72;
const CONDITIONAL_THRESHOLD: f64 = 0.42;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    AgentPrivate,
    TaskShared,
    ProjectShared,
    WorkspaceShared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Proposed,
    Approved,
    Rejected,
    Superseded,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryVector {
    /// Embedding supplied by a configured embedding provider or Qoin adapter.
    pub semantic: Vec<f32>,
    /// Explicit normalized decision dimensions in [0, 1].
    pub context: f32,
    pub outcome: f32,
    pub failure: f32,
    pub evidence: f32,
}

impl MemoryVector {
    pub fn validate(&self) -> Result<(), VectorError> {
        if self.semantic.iter().any(|v| !v.is_finite()) {
            return Err(VectorError::NonFiniteComponent);
        }
        for value in [self.context, self.outcome, self.failure, self.evidence] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(VectorError::DimensionOutOfRange);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorError {
    NonFiniteComponent,
    DimensionOutOfRange,
    DimensionMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedMemory {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub task_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub visibility: MemoryVisibility,
    pub status: MemoryStatus,
    pub vector: MemoryVector,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
    pub outcome_labels: BTreeSet<String>,
    pub source_event_ids: Vec<Uuid>,
    pub created_at_unix: i64,
    pub superseded_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub workspace_id: Uuid,
    pub requester_agent_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub semantic: Vec<f32>,
    pub context: f32,
    pub outcome: f32,
    pub failure: f32,
    pub evidence: f32,
    pub now_unix: i64,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
    pub outcome_labels: BTreeSet<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub semantic: f64,
    pub context: f64,
    pub dependency: f64,
    pub outcome: f64,
    pub failure: f64,
    pub evidence: f64,
    pub recency: f64,
    pub tags: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelevanceClass {
    Relevant,
    ConditionallyRelevant,
    Irrelevant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedMemory {
    pub memory_id: Uuid,
    pub score: ScoreBreakdown,
    pub classification: RelevanceClass,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub candidates_examined: usize,
    pub excluded_by_scope_or_status: usize,
    pub ranked: Vec<RankedMemory>,
}

/// Rank only approved, visible memories in the requested workspace. A semantic
/// vector is optional, but if present its dimensionality must match the query.
pub fn retrieve_memories(
    query: &MemoryQuery,
    memories: &[IndexedMemory],
) -> Result<RetrievalResult, VectorError> {
    if query.semantic.iter().any(|v| !v.is_finite()) {
        return Err(VectorError::NonFiniteComponent);
    }
    for value in [query.context, query.outcome, query.failure, query.evidence] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(VectorError::DimensionOutOfRange);
        }
    }
    let mut ranked = Vec::new();
    let mut excluded = 0;
    for memory in memories {
        if memory.workspace_id != query.workspace_id
            || memory.status != MemoryStatus::Approved
            || memory.superseded_by.is_some()
            || !visible_to(memory, query)
        {
            excluded += 1;
            continue;
        }
        memory.vector.validate()?;
        if !query.semantic.is_empty()
            && !memory.vector.semantic.is_empty()
            && query.semantic.len() != memory.vector.semantic.len()
        {
            return Err(VectorError::DimensionMismatch);
        }
        let score = score_memory(query, memory);
        let classification = if score.total >= RELEVANCE_THRESHOLD {
            RelevanceClass::Relevant
        } else if score.total >= CONDITIONAL_THRESHOLD {
            RelevanceClass::ConditionallyRelevant
        } else {
            RelevanceClass::Irrelevant
        };
        let mut reasons = Vec::new();
        if score.dependency > 0.0 {
            reasons.push("shares an explicit task or artifact dependency".into());
        }
        if score.failure >= 0.65 {
            reasons.push("contains a closely matching failure signal".into());
        }
        if score.semantic >= 0.75 {
            reasons.push("has strong semantic-vector similarity".into());
        }
        if score.evidence < 0.25 {
            reasons.push("historical evidence is weak".into());
        }
        ranked.push(RankedMemory {
            memory_id: memory.id,
            score,
            classification,
            reasons,
        });
    }
    ranked.sort_by(|a, b| {
        b.score
            .total
            .total_cmp(&a.score.total)
            .then_with(|| a.memory_id.as_bytes().cmp(b.memory_id.as_bytes()))
    });
    ranked.truncate(query.limit.max(1));
    Ok(RetrievalResult {
        candidates_examined: memories.len(),
        excluded_by_scope_or_status: excluded,
        ranked,
    })
}

fn visible_to(memory: &IndexedMemory, query: &MemoryQuery) -> bool {
    match memory.visibility {
        MemoryVisibility::AgentPrivate => {
            memory.owner_agent_id.is_some() && memory.owner_agent_id == query.requester_agent_id
        }
        MemoryVisibility::TaskShared => memory.task_id.is_some() && memory.task_id == query.task_id,
        MemoryVisibility::ProjectShared => {
            memory.project_id.is_some() && memory.project_id == query.project_id
        }
        MemoryVisibility::WorkspaceShared => true,
    }
}

fn score_memory(query: &MemoryQuery, memory: &IndexedMemory) -> ScoreBreakdown {
    let semantic = cosine_similarity(&query.semantic, &memory.vector.semantic);
    let context = 1.0 - (query.context as f64 - memory.vector.context as f64).abs();
    let dependency = overlap(&query.dependency_ids, &memory.dependency_ids);
    let outcome = if query.outcome_labels.is_empty() {
        1.0 - (query.outcome as f64 - memory.vector.outcome as f64).abs()
    } else {
        overlap(&query.outcome_labels, &memory.outcome_labels)
    };
    let failure = 1.0 - (query.failure as f64 - memory.vector.failure as f64).abs();
    let evidence = memory.vector.evidence as f64 * query.evidence as f64;
    let age_days = query.now_unix.saturating_sub(memory.created_at_unix).max(0) as f64 / 86_400.0;
    let recency = (1.0 / (1.0 + age_days / 30.0)).clamp(0.0, 1.0);
    let tags = overlap(&query.tags, &memory.tags);
    // Weights are explicit and explainable; tags are a small additive signal.
    let total = 0.30 * semantic
        + 0.15 * context
        + 0.18 * dependency
        + 0.10 * outcome
        + 0.12 * failure
        + 0.10 * evidence
        + 0.05 * recency
        + 0.08 * tags;
    let total = total / 1.08;
    ScoreBreakdown {
        semantic,
        context,
        dependency,
        outcome,
        failure,
        evidence,
        recency,
        tags,
        total,
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let na = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    ((dot / (na * nb) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn overlap<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalState {
    pub text: String,
    pub status: MemoryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalAction {
    Created,
    Approved,
    Edited,
    Rejected,
    Superseded,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalEvent {
    pub sequence: u64,
    pub event_id: Uuid,
    pub memory_id: Uuid,
    pub action: HistoricalAction,
    /// Full event snapshot, not a lossy summary.
    pub snapshot: HistoricalState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionReport {
    pub reconstructed: BTreeMap<Uuid, HistoricalState>,
    pub event_count: usize,
    pub issues: Vec<String>,
    pub matches_expected: Option<bool>,
}

/// Redraw a historical state by replaying immutable snapshots. Gaps are not
/// fabricated; invalid transitions are reported and excluded from the result.
pub fn reconstruct_history(
    events: &[HistoricalEvent],
    expected: Option<&BTreeMap<Uuid, HistoricalState>>,
) -> ReconstructionReport {
    let mut ordered = events.to_vec();
    ordered.sort_by_key(|event| event.sequence);
    let mut issues = Vec::new();
    let mut state = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut seen_event_ids = BTreeSet::new();
    for event in &ordered {
        if !seen_event_ids.insert(event.event_id) {
            issues.push(format!("duplicate event id {}", event.event_id));
            continue;
        }
        if !seen.insert(event.sequence) {
            issues.push(format!("duplicate event sequence {}", event.sequence));
            continue;
        }
        let exists = state.contains_key(&event.memory_id);
        let valid = match event.action {
            HistoricalAction::Created => !exists && event.snapshot.status == MemoryStatus::Proposed,
            HistoricalAction::Approved => exists && event.snapshot.status == MemoryStatus::Approved,
            HistoricalAction::Edited => exists && event.snapshot.status == MemoryStatus::Proposed,
            HistoricalAction::Rejected => exists && event.snapshot.status == MemoryStatus::Rejected,
            HistoricalAction::Superseded => {
                exists && event.snapshot.status == MemoryStatus::Superseded
            }
            HistoricalAction::Deleted => exists && event.snapshot.status == MemoryStatus::Deleted,
        };
        if !valid {
            issues.push(format!(
                "invalid {:?} transition for memory {}",
                event.action, event.memory_id
            ));
            continue;
        }
        state.insert(event.memory_id, event.snapshot.clone());
    }
    let matches_expected = expected.map(|expected_state| {
        let matches = &state == expected_state;
        if !matches {
            issues.push("reconstructed state differs from independently expected state".into());
        }
        matches
    });
    ReconstructionReport {
        reconstructed: state,
        event_count: ordered.len(),
        issues,
        matches_expected,
    }
}

#[cfg(test)]
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
        let expected = BTreeMap::from([(id, HistoricalState { text: "expected".into(), status: MemoryStatus::Proposed })]);
        let report = reconstruct_history(&events, Some(&expected));
        assert!(!report.issues.is_empty());
        assert_eq!(report.matches_expected, Some(false));
    }
}
