//! Axon's historical context architecture.
//!
//! Qoin owns semantic vector production and vector-space conventions. Axon owns
//! authorization, QKV attention, event provenance, epoch publication, Pareto
//! context selection, and task-local progressive retrieval.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QoinVector {
    pub values: Vec<f32>,
    pub model_version: String,
}
impl QoinVector {
    pub fn validate(&self) -> Result<(), AttentionError> {
        if self.values.is_empty() || self.values.iter().any(|v| !v.is_finite()) {
            return Err(AttentionError::InvalidVector);
        }
        if self.model_version.trim().is_empty() {
            return Err(AttentionError::MissingModelVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionMemory {
    pub memory_id: Uuid,
    pub key: QoinVector,
    /// The value is a pointer/payload reference, never a lossy replacement for
    /// the immutable event record.
    pub value: String,
    pub outcome_score: f32,
    pub evidence_score: f32,
    pub source_event_sequence: i64,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionQuery {
    pub query: QoinVector,
    pub heads: usize,
    pub tags: BTreeSet<String>,
    pub dependency_ids: BTreeSet<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionScore {
    pub memory_id: Uuid,
    pub attention: f64,
    pub outcome: f64,
    pub evidence: f64,
    pub tag_overlap: f64,
    pub dependency_overlap: f64,
    pub total: f64,
    pub source_event_sequence: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoPolicy {
    /// The 80/20 principle is a budget heuristic, not a guaranteed law.
    pub initial_share: f64,
    pub target_contribution_coverage: f64,
}
impl Default for ParetoPolicy {
    fn default() -> Self {
        Self { initial_share: 0.20, target_contribution_coverage: 0.80 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoSelection {
    pub initial: Vec<AttentionScore>,
    pub deferred: Vec<AttentionScore>,
    pub achieved_coverage: f64,
    pub budget_exhausted: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttentionError {
    #[error("query and key vectors must be non-empty, finite, and dimensionally equal")]
    InvalidVector,
    #[error("query dimension must be divisible by the number of attention heads")]
    InvalidHeadCount,
    #[error("vector model version is required")]
    MissingModelVersion,
    #[error("memory score metadata must be finite and in [0, 1]")]
    InvalidScoreMetadata,
    #[error("Pareto policy values must be in (0, 1]")]
    InvalidParetoPolicy,
}

/// Multi-head scaled dot-product attention over Qoin keys. Each head attends
/// to a disjoint vector slice; head probabilities are averaged. The returned
/// value is a ranked set of source references, not synthesized chat content.
pub fn qkv_attention(query: &AttentionQuery, memories: &[AttentionMemory]) -> Result<Vec<AttentionScore>, AttentionError> {
    query.query.validate()?;
    if query.heads == 0 || query.query.values.len() % query.heads != 0 {
        return Err(AttentionError::InvalidHeadCount);
    }
    let mut scored = Vec::with_capacity(memories.len());
    for memory in memories {
        memory.key.validate()?;
        if memory.key.values.len() != query.query.values.len() {
            return Err(AttentionError::InvalidVector);
        }
        if !memory.outcome_score.is_finite() || !(0.0..=1.0).contains(&memory.outcome_score)
            || !memory.evidence_score.is_finite() || !(0.0..=1.0).contains(&memory.evidence_score) {
            return Err(AttentionError::InvalidScoreMetadata);
        }
        let width = query.query.values.len() / query.heads;
        let mut attention = 0.0;
        for head in 0..query.heads {
            let start = head * width;
            let end = start + width;
            let dot = query.query.values[start..end].iter()
                .zip(&memory.key.values[start..end])
                .map(|(q, k)| *q as f64 * *k as f64).sum::<f64>();
            attention += dot / (width as f64).sqrt();
        }
        attention /= query.heads as f64;
        let tag_overlap = jaccard(&query.tags, &memory.tags);
        let dependency_overlap = jaccard(&query.dependency_ids, &memory.dependency_ids);
        // Attention is a logit; sigmoid maps it to [0, 1] before combining
        // independently observed outcome and evidence signals.
        let attention = sigmoid(attention);
        let outcome = memory.outcome_score as f64;
        let evidence = memory.evidence_score as f64;
        let total = 0.65 * attention + 0.15 * outcome + 0.10 * evidence
            + 0.05 * tag_overlap + 0.05 * dependency_overlap;
        scored.push(AttentionScore { memory_id: memory.memory_id, attention, outcome, evidence,
            tag_overlap, dependency_overlap, total, source_event_sequence: memory.source_event_sequence });
    }
    scored.sort_by(|a,b| b.total.total_cmp(&a.total)
        .then_with(|| b.source_event_sequence.cmp(&a.source_event_sequence))
        .then_with(|| a.memory_id.as_bytes().cmp(b.memory_id.as_bytes())));
    Ok(scored)
}

/// Select the highest-contribution initial context under a strict context
/// budget. The target coverage is measured over non-negative ranked scores;
/// when the budget cannot reach 80%, the achieved coverage is reported rather
/// than pretending the Pareto rule was satisfied.
pub fn pareto_select(ranked: &[AttentionScore], policy: &ParetoPolicy) -> Result<ParetoSelection, AttentionError> {
    if !(0.0..1.0).contains(&policy.initial_share)
        || !(0.0..1.0).contains(&policy.target_contribution_coverage) {
        return Err(AttentionError::InvalidParetoPolicy);
    }
    if ranked.is_empty() {
        return Ok(ParetoSelection { initial: vec![], deferred: vec![], achieved_coverage: 1.0, budget_exhausted: false });
    }
    let total: f64 = ranked.iter().map(|s| s.total.max(0.0)).sum();
    let budget = ((ranked.len() as f64 * policy.initial_share).ceil() as usize).max(1).min(ranked.len());
    let mut selected = Vec::new();
    let mut contribution = 0.0;
    for score in ranked.iter().take(budget) {
        selected.push(score.clone());
        contribution += score.total.max(0.0);
        if total == 0.0 || contribution / total >= policy.target_contribution_coverage { break; }
    }
    let selected_ids: BTreeSet<_> = selected.iter().map(|s| s.memory_id).collect();
    let deferred = ranked.iter().filter(|s| !selected_ids.contains(&s.memory_id)).cloned().collect();
    let achieved = if total == 0.0 { 1.0 } else { (contribution / total).clamp(0.0, 1.0) };
    Ok(ParetoSelection { budget_exhausted: achieved < policy.target_contribution_coverage,
        initial: selected, deferred, achieved_coverage: achieved })
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 { 1.0 / (1.0 + (-value).exp()) }
    else { let e = value.exp(); e / (1.0 + e) }
}
fn jaccard<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn q(values: Vec<f32>) -> QoinVector { QoinVector { values, model_version: "qoin-test-v1".into() } }
    fn memory(id: Uuid, values: Vec<f32>, outcome: f32) -> AttentionMemory {
        AttentionMemory { memory_id: id, key: q(values), value: format!("memory:{id}"),
            outcome_score: outcome, evidence_score: 0.9, source_event_sequence: 1,
            tags: BTreeSet::new(), dependency_ids: BTreeSet::new() }
    }
    #[test]
    fn multi_head_attention_ranks_matching_keys() {
        let id = Uuid::new_v4();
        let query = AttentionQuery { query: q(vec![1.0, 0.0, 1.0, 0.0]), heads: 2,
            tags: BTreeSet::new(), dependency_ids: BTreeSet::new() };
        let ranked = qkv_attention(&query, &[memory(Uuid::new_v4(), vec![0.0, 1.0, 0.0, 1.0], 1.0),
            memory(id, vec![1.0, 0.0, 1.0, 0.0], 0.8)]).unwrap();
        assert_eq!(ranked[0].memory_id, id);
    }
    #[test]
    fn pareto_selection_respects_budget_and_reports_real_coverage() {
        let scores = (0..10).map(|i| AttentionScore { memory_id: Uuid::new_v4(),
            attention: 0.0, outcome: 0.0, evidence: 0.0, tag_overlap: 0.0,
            dependency_overlap: 0.0, total: if i == 0 { 9.0 } else { 1.0 },
            source_event_sequence: i }).collect::<Vec<_>>();
        let selection = pareto_select(&scores, &ParetoPolicy::default()).unwrap();
        assert_eq!(selection.initial.len(), 2);
        assert!(selection.achieved_coverage >= 0.8);
        assert_eq!(selection.deferred.len(), 8);
    }
}
