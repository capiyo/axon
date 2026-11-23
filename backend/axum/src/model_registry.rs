use crate::{
    audit::AuditEvent,
    storage::{EntityKind, Storage, StorageError, StorageMutation, StoredEntity},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryStatus {
    Candidate,
    Approved,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderDataTerms {
    pub processing_regions: Vec<String>,
    pub trains_on_api_inputs: Option<bool>,
    pub retention_days: Option<u32>,
    pub retention_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProvider {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub model_discovery_url: Option<String>,
    pub pricing_source_url: Option<String>,
    pub api_key_env: Option<String>,
    pub discovery_enabled: bool,
    pub rate_limit_per_minute: u16,
    pub status: String,
    pub data_terms: ProviderDataTerms,
    pub terms_reviewed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRecord {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub model_name: String,
    pub status: RegistryStatus,
    pub abilities: BTreeSet<String>,
    pub categories: BTreeSet<String>,
    pub context_length: u64,
    pub input_cost_per_million: u64,
    pub output_cost_per_million: u64,
    pub typical_latency_ms: u64,
    pub regions: BTreeSet<String>,
    pub version: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub notes: String,
    pub curated_rank: BTreeMap<String, u16>,
    pub healthy: bool,
    pub flagged_for_review: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelChangeFlag {
    pub id: Uuid,
    pub model_id: Uuid,
    pub change_kind: String,
    pub previous_value: Option<String>,
    pub observed_value: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub cleared_at: Option<DateTime<Utc>>,
    pub cleared_by: Option<Uuid>,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEvaluation {
    pub id: Uuid,
    pub model_id: Uuid,
    pub category: String,
    pub test_set_version: String,
    pub score: f64,
    pub run_at: DateTime<Utc>,
    pub cost_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelStat {
    pub id: Uuid,
    pub model_id: Uuid,
    pub category: String,
    pub role: AssignmentRole,
    pub runs: u64,
    pub completion_rate: f64,
    pub supervisor_pass_rate: f64,
    pub user_retry_rate: f64,
    pub cost_per_run_micros: u64,
    pub latency_ms: u64,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentRole {
    ChecklistWriter,
    Worker,
    Supervisor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLevel {
    Quick,
    Normal,
    Important,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyRequirement {
    pub forbid_training: bool,
    pub forbid_retention: bool,
    pub allowed_regions: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct AssignmentProfile {
    pub run_id: Uuid,
    pub category: String,
    pub level: TaskLevel,
    pub low_risk: bool,
    pub required_abilities: BTreeSet<String>,
    pub required_context_tokens: u64,
    pub privacy: PrivacyRequirement,
    pub estimated_input_tokens: u64,
    pub estimated_output_tokens: u64,
    pub per_task_budget_micros: u64,
    pub daily_budget_remaining_micros: u64,
    pub exploration_percent: u8,
    pub minimum_history_runs: u64,
    pub curated_model_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignmentDecision {
    pub id: Uuid,
    pub run_id: Uuid,
    pub role: AssignmentRole,
    pub slot: u8,
    pub candidates_considered: Vec<Uuid>,
    pub filters_applied: Vec<String>,
    pub chosen_model_id: Uuid,
    pub reason: String,
    pub estimated_cost_micros: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignmentPlan {
    pub level: TaskLevel,
    pub decisions: Vec<AssignmentDecision>,
    pub estimated_cost_micros: u64,
    pub budget_downgraded: bool,
    pub exploration_used: bool,
    pub diversity_degraded: bool,
    pub notices: Vec<String>,
}

pub trait ModelAssigner: Send + Sync {
    fn assign(
        &self,
        profile: &AssignmentProfile,
        providers: &[ModelProvider],
        models: &[ModelRecord],
        stats: &[ModelStat],
    ) -> Result<AssignmentPlan, AssignmentError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicModelAssigner;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AssignmentError {
    #[error("no approved, healthy model satisfies the task's category, ability, context and privacy requirements")]
    NoEligibleModels,
    #[error("provider diversity requirements cannot be met at this supervision level")]
    DiversityUnavailable,
    #[error("configured budgets cannot fund even the Quick assignment")]
    BudgetExceeded,
    #[error("invalid assignment configuration")]
    InvalidConfiguration,
}

#[derive(Debug, Clone)]
struct Candidate<'a> {
    model: &'a ModelRecord,
    provider: &'a ModelProvider,
    stat: Option<&'a ModelStat>,
    estimated_cost: u64,
    score: f64,
}

impl ModelAssigner for DeterministicModelAssigner {
    fn assign(
        &self,
        profile: &AssignmentProfile,
        providers: &[ModelProvider],
        models: &[ModelRecord],
        stats: &[ModelStat],
    ) -> Result<AssignmentPlan, AssignmentError> {
        assign_models(profile, providers, models, stats)
    }
}

fn level_counts(level: &TaskLevel) -> (usize, usize, usize) {
    match level {
        TaskLevel::Quick => (0, 1, 0),
        TaskLevel::Normal => (1, 1, 1),
        TaskLevel::Important => (2, 2, 2),
    }
}

fn estimate_cost(model: &ModelRecord, input: u64, output: u64) -> u64 {
    let input_cost = (u128::from(model.input_cost_per_million) * u128::from(input)) / 1_000_000;
    let output_cost = (u128::from(model.output_cost_per_million) * u128::from(output)) / 1_000_000;
    input_cost
        .saturating_add(output_cost)
        .min(u128::from(u64::MAX)) as u64
}

fn privacy_allows(provider: &ModelProvider, requirement: &PrivacyRequirement) -> bool {
    if requirement.forbid_training && provider.data_terms.trains_on_api_inputs != Some(false) {
        return false;
    }
    if requirement.forbid_retention && provider.data_terms.retention_days != Some(0) {
        return false;
    }
    if !requirement.allowed_regions.is_empty()
        && !provider
            .data_terms
            .processing_regions
            .iter()
            .any(|region| requirement.allowed_regions.contains(region))
    {
        return false;
    }
    true
}

fn score_candidate(
    stat: Option<&ModelStat>,
    level: &TaskLevel,
    minimum_history_runs: u64,
    curated_rank: u16,
) -> f64 {
    use crate::vector_decision::{AgentVector, TaskVector, VectorDecisionEngine};
    let prior = (1000.0 - f64::from(curated_rank.min(1000))) / 1000.0;
    let Some(stat) = stat.filter(|item| item.runs >= minimum_history_runs) else {
        return prior * 1000.0;
    };
    let Ok(agent_vector) = AgentVector::from_outcomes(
        stat.completion_rate,
        stat.supervisor_pass_rate,
        stat.latency_ms,
        stat.cost_per_run_micros,
        stat.user_retry_rate,
        stat.runs,
        minimum_history_runs,
    ) else {
        return prior * 1000.0;
    };
    VectorDecisionEngine
        .score(agent_vector, TaskVector::for_level(level), prior)
        .map(|score| score * 1000.0)
        .unwrap_or(prior * 1000.0)
}

