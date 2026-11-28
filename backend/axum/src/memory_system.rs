//! Scoped agent memory and deterministic context allocation.
//!
//! Memory is not copied indiscriminately between agents. Every retrieval is
//! filtered by workspace, scope, sensitivity and explicit agent grants before
//! utility scoring or token allocation takes place.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Workspace,
    Task,
    Agent,
    Execution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySensitivity {
    Public,
    Internal,
    Confidential,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    Unverified,
    Proposed,
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub scope: MemoryScope,
    pub scope_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub content: String,
    pub source: String,
    pub confidence: f64,
    pub authority: f64,
    pub sensitivity: MemorySensitivity,
    pub verification: Verification,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub version: u64,
    pub token_cost: usize,
}

impl MemoryRecord {
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.content.trim().is_empty()
            || self.source.trim().is_empty()
            || self.token_cost == 0
            || self.token_cost > i64::MAX as usize
            || self.version == 0
            || self.version > i64::MAX as u64
        {
            return Err(MemoryError::InvalidRecord);
        }
        if !self.confidence.is_finite()
            || !self.authority.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || !(0.0..=1.0).contains(&self.authority)
        {
            return Err(MemoryError::InvalidScore);
        }
        match self.scope {
            MemoryScope::Workspace if self.scope_id.is_some() || self.owner_agent_id.is_some() => {
                Err(MemoryError::InvalidScope)
            }
            MemoryScope::Task | MemoryScope::Execution if self.scope_id.is_none() => {
                Err(MemoryError::InvalidScope)
            }
            MemoryScope::Agent if self.owner_agent_id.is_none() => Err(MemoryError::InvalidScope),
            MemoryScope::Execution if self.expires_at.is_none() => Err(MemoryError::InvalidScope),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalContext {
    pub workspace_id: Uuid,
    pub task_id: Option<Uuid>,
    pub agent_id: Uuid,
    pub allowed_sensitivity: MemorySensitivity,
    pub granted_memory_ids: BTreeSet<Uuid>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryVector {
    pub relevance: f64,
    pub freshness: f64,
    pub authority: f64,
    pub confidence: f64,
    pub verification: f64,
    pub sensitivity_penalty: f64,
}

impl MemoryVector {
    pub fn utility(self) -> f64 {
        // Verified facts dominate; stale, unverified or sensitive records
        // consume context only when their relevance justifies the cost.
        self.relevance.clamp(0.0, 1.0) * 0.35
            + self.freshness.clamp(0.0, 1.0) * 0.15
            + self.authority.clamp(0.0, 1.0) * 0.15
            + self.confidence.clamp(0.0, 1.0) * 0.15
            + self.verification.clamp(0.0, 1.0) * 0.20
            - self.sensitivity_penalty.clamp(0.0, 1.0) * 0.25
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AllocatedMemory {
    pub memory_id: Uuid,
    pub token_cost: usize,
    pub utility: f64,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryAllocation {
    pub workspace_id: Uuid,
    pub task_id: Option<Uuid>,
    pub agent_id: Uuid,
    pub token_budget: usize,
    pub tokens_allocated: usize,
    pub selected: Vec<AllocatedMemory>,
    pub excluded: Vec<Uuid>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryError {
    #[error("memory record is invalid")]
    InvalidRecord,
    #[error("memory scores must be finite values between zero and one")]
    InvalidScore,
    #[error("memory scope identifiers are inconsistent")]
    InvalidScope,
    #[error("agent is not authorized to access this memory")]
    AccessDenied,
    #[error("memory belongs to another workspace")]
    WorkspaceMismatch,
    #[error("context token budget must be greater than zero")]
    InvalidBudget,
}

fn sensitivity_rank(value: MemorySensitivity) -> u8 {
    match value {
        MemorySensitivity::Public => 0,
        MemorySensitivity::Internal => 1,
        MemorySensitivity::Confidential => 2,
        MemorySensitivity::Secret => 3,
    }
}

pub fn can_read(record: &MemoryRecord, context: &RetrievalContext) -> bool {
    if record.workspace_id != context.workspace_id
        || record
            .expires_at
            .is_some_and(|expiry| expiry <= context.now)
        || sensitivity_rank(record.sensitivity) > sensitivity_rank(context.allowed_sensitivity)
        || record.verification == Verification::Rejected
    {
        return false;
    }
    if record.verification != Verification::Verified
        && !(record.scope == MemoryScope::Agent && record.owner_agent_id == Some(context.agent_id))
    {
        return false;
    }
    match record.scope {
        MemoryScope::Workspace => true,
        MemoryScope::Task => record.scope_id == context.task_id,
        MemoryScope::Agent => {
            record.owner_agent_id == Some(context.agent_id)
                || context.granted_memory_ids.contains(&record.id)
        }
        MemoryScope::Execution => {
            record.scope_id == context.task_id && context.granted_memory_ids.contains(&record.id)
        }
    }
}

/// Deterministic lexical relevance used when no embedding provider is
/// configured. It intentionally returns zero for unrelated records so
/// freshness or authority alone cannot pull irrelevant memory into context.
pub fn lexical_relevance(query: &str, content: &str) -> f64 {
    fn terms(value: &str) -> BTreeSet<String> {
        value
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|term| term.chars().count() > 2)
            .map(str::to_lowercase)
            .collect()
    }
    let query_terms = terms(query);
    if query_terms.is_empty() {
        return 0.0;
    }
    let content_terms = terms(content);
    let overlap = query_terms.intersection(&content_terms).count() as f64;
    let coverage = overlap / query_terms.len() as f64;
    let phrase_bonus = if content.to_lowercase().contains(&query.to_lowercase()) {
        0.25
    } else {
        0.0
    };
    (coverage + phrase_bonus).min(1.0)
}

pub fn score_memory(record: &MemoryRecord, relevance: f64, now: DateTime<Utc>) -> MemoryVector {
    let age_seconds = now
        .signed_duration_since(record.created_at)
        .num_seconds()
        .max(0) as f64;
    let freshness = 1.0 / (1.0 + age_seconds / (30.0 * 24.0 * 60.0 * 60.0));
    let verification = match record.verification {
        Verification::Verified => 1.0,
        Verification::Proposed => 0.45,
        Verification::Unverified => 0.2,
        Verification::Rejected => 0.0,
    };
    let sensitivity_penalty = match record.sensitivity {
        MemorySensitivity::Public => 0.0,
        MemorySensitivity::Internal => 0.1,
        MemorySensitivity::Confidential => 0.35,
        MemorySensitivity::Secret => 1.0,
    };
    MemoryVector {
        relevance,
        freshness,
        authority: record.authority,
        confidence: record.confidence,
        verification,
        sensitivity_penalty,
    }
}

pub fn allocate_context(
    records: &[MemoryRecord],
    context: &RetrievalContext,
    relevance: &[(Uuid, f64)],
    token_budget: usize,
) -> Result<MemoryAllocation, MemoryError> {
    if token_budget == 0 || token_budget > i64::MAX as usize {
        return Err(MemoryError::InvalidBudget);
    }
    let relevance_map: std::collections::BTreeMap<_, _> = relevance.iter().copied().collect();
    let mut ranked = Vec::new();
    let mut excluded = Vec::new();
    for record in records {
        record.validate()?;
        if !can_read(record, context) {
            excluded.push(record.id);
            continue;
        }
        let Some(relevance) = relevance_map.get(&record.id).copied() else {
            excluded.push(record.id);
            continue;
        };
        if !relevance.is_finite() || !(0.0..=1.0).contains(&relevance) {
            return Err(MemoryError::InvalidScore);
        }
        if relevance == 0.0 {
            excluded.push(record.id);
            continue;
        }
        let utility = score_memory(record, relevance, context.now).utility();
        ranked.push((
            record.id,
            record.token_cost,
            utility,
            record.content.clone(),
            record.source.clone(),
            record.verification,
        ));
    }
    // Greedy utility density is deterministic and bounded. Stable UUID tie
    // breaking makes identical inputs produce byte-for-byte equivalent plans.
    ranked.sort_by(|a, b| {
        let left = a.2 / a.1.max(1) as f64;
        let right = b.2 / b.1.max(1) as f64;
        right.total_cmp(&left).then_with(|| a.0.cmp(&b.0))
    });
    let mut tokens_allocated = 0usize;
    let mut selected = Vec::new();
    for (id, cost, utility, content, source, verification) in ranked {
        if cost > token_budget.saturating_sub(tokens_allocated) {
            excluded.push(id);
            continue;
        }
        tokens_allocated += cost;
        selected.push(AllocatedMemory {
            memory_id: id,
            token_cost: cost,
            utility,
            rank: selected.len() + 1,
        });
    }
    excluded.sort();
    excluded.dedup();
    Ok(MemoryAllocation {
        workspace_id: context.workspace_id,
        task_id: context.task_id,
        agent_id: context.agent_id,
        token_budget,
        tokens_allocated,
        selected,
        excluded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn record(workspace: Uuid, scope: MemoryScope, sensitivity: MemorySensitivity) -> MemoryRecord {
        MemoryRecord {
            id: Uuid::new_v4(),
            workspace_id: workspace,
            scope,
            scope_id: match scope {
                MemoryScope::Task | MemoryScope::Execution => Some(Uuid::new_v4()),
                _ => None,
            },
            owner_agent_id: (scope == MemoryScope::Agent).then(Uuid::new_v4),
            content: "A durable project decision".into(),
            source: "supervisor-run:artifact-1".into(),
            confidence: 0.9,
            authority: 0.9,
            sensitivity,
            verification: Verification::Verified,
            created_at: Utc::now(),
            expires_at: None,
            version: 1,
            token_cost: 100,
        }
    }

    #[test]
    fn workspace_isolation_and_agent_private_memory_are_enforced() {
        let workspace = Uuid::new_v4();
        let agent = Uuid::new_v4();
        let private = record(workspace, MemoryScope::Agent, MemorySensitivity::Internal);
        let mut context = RetrievalContext {
            workspace_id: workspace,
            task_id: None,
            agent_id: agent,
            allowed_sensitivity: MemorySensitivity::Confidential,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        assert!(!can_read(&private, &context));
        context.granted_memory_ids.insert(private.id);
        assert!(can_read(&private, &context));
        context.workspace_id = Uuid::new_v4();
        assert!(!can_read(&private, &context));
    }

    #[test]
    fn expired_and_over_sensitivity_records_never_enter_context() {
        let workspace = Uuid::new_v4();
        let mut secret = record(workspace, MemoryScope::Workspace, MemorySensitivity::Secret);
        secret.expires_at = Some(Utc::now() - Duration::seconds(1));
        let context = RetrievalContext {
            workspace_id: workspace,
            task_id: None,
            agent_id: Uuid::new_v4(),
            allowed_sensitivity: MemorySensitivity::Internal,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        assert!(!can_read(&secret, &context));
    }

    #[test]
    fn allocation_never_exceeds_budget_and_is_deterministic() {
        let workspace = Uuid::new_v4();
        let first = record(
            workspace,
            MemoryScope::Workspace,
            MemorySensitivity::Internal,
        );
        let mut second = record(
            workspace,
            MemoryScope::Workspace,
            MemorySensitivity::Internal,
        );
        second.token_cost = 250;
        let context = RetrievalContext {
            workspace_id: workspace,
            task_id: None,
            agent_id: Uuid::new_v4(),
            allowed_sensitivity: MemorySensitivity::Confidential,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        let relevance = vec![(first.id, 1.0), (second.id, 0.9)];
        let a =
            allocate_context(&[first.clone(), second.clone()], &context, &relevance, 200).unwrap();
        let b = allocate_context(&[second, first], &context, &relevance, 200).unwrap();
        assert!(a.tokens_allocated <= a.token_budget);
        assert_eq!(a, b);
    }

    #[test]
    fn lexical_relevance_excludes_unrelated_memory_and_rewards_exact_phrases() {
        assert_eq!(
            lexical_relevance("portfolio risk model", "football fixture commentary"),
            0.0
        );
        assert!(lexical_relevance("portfolio risk model", "portfolio risk model design") > 0.7);
    }

    #[test]
    fn proposed_shared_memory_waits_for_human_verification() {
        let workspace = Uuid::new_v4();
        let mut item = record(
            workspace,
            MemoryScope::Workspace,
            MemorySensitivity::Internal,
        );
        item.verification = Verification::Proposed;
        let context = RetrievalContext {
            workspace_id: workspace,
            task_id: None,
            agent_id: Uuid::new_v4(),
            allowed_sensitivity: MemorySensitivity::Internal,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        assert!(!can_read(&item, &context));
        item.verification = Verification::Verified;
        assert!(can_read(&item, &context));
    }

    #[test]
    fn task_memory_is_confined_to_its_task() {
        let workspace = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let mut item = record(workspace, MemoryScope::Task, MemorySensitivity::Internal);
        item.scope_id = Some(task_id);
        let mut context = RetrievalContext {
            workspace_id: workspace,
            task_id: Some(task_id),
            agent_id: Uuid::new_v4(),
            allowed_sensitivity: MemorySensitivity::Internal,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        assert!(can_read(&item, &context));
        context.task_id = Some(Uuid::new_v4());
        assert!(!can_read(&item, &context));
    }

    #[test]
    fn execution_memory_requires_an_expiry() {
        let mut item = record(
            Uuid::new_v4(),
            MemoryScope::Execution,
            MemorySensitivity::Internal,
        );
        item.expires_at = None;
        assert_eq!(item.validate(), Err(MemoryError::InvalidScope));
    }

    #[test]
    fn invalid_scores_are_rejected() {
        let mut item = record(
            Uuid::new_v4(),
            MemoryScope::Workspace,
            MemorySensitivity::Public,
        );
        item.confidence = f64::NAN;
        assert_eq!(item.validate(), Err(MemoryError::InvalidScore));
    }
}
