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

pub fn score_candidate(
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

fn candidates<'a>(
    profile: &AssignmentProfile,
    providers: &'a [ModelProvider],
    models: &'a [ModelRecord],
    stats: &'a [ModelStat],
    level: &TaskLevel,
    role: &AssignmentRole,
) -> Vec<Candidate<'a>> {
    let provider_map: BTreeMap<_, _> = providers
        .iter()
        .map(|provider| (provider.id, provider))
        .collect();
    let curated: BTreeMap<_, _> = profile
        .curated_model_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index as u16))
        .collect();
    let mut result = Vec::new();
    for model in models {
        if model.status != RegistryStatus::Approved || !model.healthy || model.flagged_for_review {
            continue;
        }
        if !model.categories.contains(&profile.category)
            || !profile
                .required_abilities
                .iter()
                .all(|ability| model.abilities.contains(ability))
            || model.context_length < profile.required_context_tokens
        {
            continue;
        }
        let Some(provider) = provider_map.get(&model.provider_id).copied() else {
            continue;
        };
        if provider.status != "active" || !privacy_allows(provider, &profile.privacy) {
            continue;
        }
        let stat = stats.iter().find(|item| {
            item.model_id == model.id && item.category == profile.category && item.role == *role
        });
        let configured_rank = curated
            .get(&model.id)
            .copied()
            .or_else(|| model.curated_rank.get(&profile.category).copied());
        let history_qualified = stat.is_some_and(|item| item.runs >= profile.minimum_history_runs);
        if configured_rank.is_none() && !history_qualified {
            continue;
        }
        let rank = configured_rank.unwrap_or(0);
        let score = score_candidate(stat, level, profile.minimum_history_runs, rank);
        result.push(Candidate {
            model,
            provider,
            stat,
            estimated_cost: estimate_cost(
                model,
                profile.estimated_input_tokens,
                profile.estimated_output_tokens,
            ),
            score,
        });
    }
    result.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.estimated_cost.cmp(&right.estimated_cost))
            .then_with(|| left.model.id.cmp(&right.model.id))
    });
    result
}

fn exploration_candidate<'a>(
    pool: &'a [Candidate<'a>],
    chosen: &[Uuid],
    profile: &AssignmentProfile,
    level: &TaskLevel,
) -> Option<&'a Candidate<'a>> {
    if *level != TaskLevel::Quick || !profile.low_risk || profile.exploration_percent == 0 {
        return None;
    }
    let bucket = (profile.run_id.as_u128() % 100) as u8;
    if bucket >= profile.exploration_percent.min(100) {
        return None;
    }
    pool.iter()
        .filter(|candidate| !chosen.contains(&candidate.model.id))
        .filter(|candidate| Utc::now() - candidate.model.last_seen_at < Duration::days(30))
        .max_by(|left, right| left.score.total_cmp(&right.score))
}

pub fn assign_models(
    profile: &AssignmentProfile,
    providers: &[ModelProvider],
    models: &[ModelRecord],
    stats: &[ModelStat],
) -> Result<AssignmentPlan, AssignmentError> {
    if profile.exploration_percent > 100
        || profile.minimum_history_runs == 0
        || (!profile.curated_model_ids.is_empty()
            && !(4..=6).contains(&profile.curated_model_ids.len()))
    {
        return Err(AssignmentError::InvalidConfiguration);
    }
    let mut level = profile.level.clone();
    let mut notices = Vec::new();
    let mut budget_downgraded = false;
    let mut diversity_degraded = false;

    loop {
        let (writers, workers, supervisors) = level_counts(&level);
        let mut decisions = Vec::new();
        let mut writer_models = Vec::new();
        let mut worker_models = Vec::new();
        let mut supervisor_models = Vec::new();
        let mut writer_providers = BTreeSet::new();
        let mut worker_providers = BTreeSet::new();
        let mut supervisor_providers = BTreeSet::new();
        let mut estimated_total = 0u64;
        let mut failure = None;

        for (role, count) in [
            (AssignmentRole::ChecklistWriter, writers),
            (AssignmentRole::Worker, workers),
            (AssignmentRole::Supervisor, supervisors),
        ] {
            let pool = candidates(profile, providers, models, stats, &level, &role);
            if pool.is_empty() {
                failure = Some(AssignmentError::NoEligibleModels);
                break;
            }
            for slot in 0..count {
                let chosen = if role == AssignmentRole::Worker {
                    pool.iter().find(|candidate| {
                        !worker_models.contains(&candidate.model.id)
                            && !worker_providers.contains(&candidate.provider.id)
                    })
                } else if role == AssignmentRole::Supervisor {
                    pool.iter().find(|candidate| {
                        !supervisor_models.contains(&candidate.model.id)
                            && !worker_providers.contains(&candidate.provider.id)
                            && !supervisor_providers.contains(&candidate.provider.id)
                    })
                } else {
                    pool.iter()
                        .find(|candidate| {
                            !writer_models.contains(&candidate.model.id)
                                && !writer_providers.contains(&candidate.provider.id)
                        })
                        .or_else(|| {
                            pool.iter()
                                .find(|candidate| !writer_models.contains(&candidate.model.id))
                        })
                };
                let Some(chosen) = chosen else {
                    if role == AssignmentRole::ChecklistWriter {
                        diversity_degraded = true;
                        continue;
                    }
                    failure = Some(AssignmentError::DiversityUnavailable);
                    break;
                };
                let cost = chosen.estimated_cost;
                if estimated_total.saturating_add(cost) > profile.per_task_budget_micros
                    || estimated_total.saturating_add(cost) > profile.daily_budget_remaining_micros
                {
                    failure = Some(AssignmentError::BudgetExceeded);
                    break;
                }
                let considered = pool.iter().map(|candidate| candidate.model.id).collect();
                match role {
                    AssignmentRole::ChecklistWriter => {
                        writer_models.push(chosen.model.id);
                        writer_providers.insert(chosen.provider.id);
                    }
                    AssignmentRole::Worker => {
                        worker_models.push(chosen.model.id);
                        worker_providers.insert(chosen.provider.id);
                    }
                    AssignmentRole::Supervisor => {
                        supervisor_models.push(chosen.model.id);
                        supervisor_providers.insert(chosen.provider.id);
                    }
                }
                estimated_total = estimated_total.saturating_add(cost);
                decisions.push(AssignmentDecision {
                    id: Uuid::new_v4(),
                    run_id: profile.run_id,
                    role: role.clone(),
                    slot: slot as u8,
                    candidates_considered: considered,
                    filters_applied: vec![
                        "approved_only".into(),
                        "healthy_only".into(),
                        "category".into(),
                        "abilities".into(),
                        "context_length".into(),
                        "privacy_terms".into(),
                        "provider_active".into(),
                    ],
                    chosen_model_id: chosen.model.id,
                    reason: if chosen
                        .stat
                        .is_some_and(|stat| stat.runs >= profile.minimum_history_runs)
                    {
                        "Selected by category performance, cost and latency history.".into()
                    } else {
                        "Selected by the configured curated cold-start ranking.".into()
                    },
                    estimated_cost_micros: cost,
                    created_at: Utc::now(),
                });
            }
            if failure.is_some() {
                break;
            }
        }

        if failure.is_none() {
            let mut exploration_used = false;
            if level == TaskLevel::Quick && profile.low_risk {
                let pool = candidates(
                    profile,
                    providers,
                    models,
                    stats,
                    &level,
                    &AssignmentRole::Worker,
                );
                if let Some(exploration) =
                    exploration_candidate(&pool, &worker_models, profile, &level)
                {
                    if let Some(decision) = decisions
                        .iter_mut()
                        .find(|decision| decision.role == AssignmentRole::Worker)
                    {
                        let revised_total = estimated_total
                            .saturating_sub(decision.estimated_cost_micros)
                            .saturating_add(exploration.estimated_cost);
                        if revised_total <= profile.per_task_budget_micros
                            && revised_total <= profile.daily_budget_remaining_micros
                        {
                            decision.candidates_considered =
                                pool.iter().map(|candidate| candidate.model.id).collect();
                            decision
                                .filters_applied
                                .push("low_risk_quick_exploration".into());
                            decision.chosen_model_id = exploration.model.id;
                            decision.reason = "Approved recently-seen model selected by configured low-risk exploration.".into();
                            decision.estimated_cost_micros = exploration.estimated_cost;
                            estimated_total = revised_total;
                            exploration_used = true;
                        }
                    }
                }
            }
            if budget_downgraded {
                notices.push("The requested supervision level exceeded the configured budget; Axon selected a lower-cost level.".into());
            }
            if diversity_degraded {
                notices.push("Checklist writers could not be assigned distinct providers; writer diversity was reduced.".into());
            }
            return Ok(AssignmentPlan {
                level,
                decisions,
                estimated_cost_micros: estimated_total,
                budget_downgraded,
                exploration_used,
                diversity_degraded,
                notices,
            });
        }

        match failure.expect("failure checked") {
            AssignmentError::BudgetExceeded if level != TaskLevel::Quick => {
                level = match level {
                    TaskLevel::Important => TaskLevel::Normal,
                    TaskLevel::Normal => TaskLevel::Quick,
                    TaskLevel::Quick => unreachable!(),
                };
                budget_downgraded = true;
            }
            AssignmentError::DiversityUnavailable | AssignmentError::NoEligibleModels
                if level != TaskLevel::Quick =>
            {
                level = match level {
                    TaskLevel::Important => TaskLevel::Normal,
                    TaskLevel::Normal => TaskLevel::Quick,
                    TaskLevel::Quick => unreachable!(),
                };
                diversity_degraded = true;
                notices.push(
                    "Eligible models or provider diversity could not satisfy the requested supervision level; Axon reduced the supervision level.".into(),
                );
            }
            other => return Err(other),
        }
    }
}

/// Return the next eligible model after a provider timeout/failure. The caller
/// supplies provider IDs already used by other roles so fallback cannot break
/// the worker/supervisor separation rules.
pub fn fallback_model(
    profile: &AssignmentProfile,
    providers: &[ModelProvider],
    models: &[ModelRecord],
    stats: &[ModelStat],
    role: &AssignmentRole,
    excluded_model_ids: &BTreeSet<Uuid>,
    forbidden_provider_ids: &BTreeSet<Uuid>,
) -> Option<Uuid> {
    candidates(profile, providers, models, stats, &profile.level, role)
        .into_iter()
        .find(|candidate| {
            !excluded_model_ids.contains(&candidate.model.id)
                && !forbidden_provider_ids.contains(&candidate.provider.id)
        })
        .map(|candidate| candidate.model.id)
}

/// Replace a failed assignment with the next eligible model while preserving
/// role/provider diversity. Call only after a provider timeout or hard failure.
pub fn recover_assignment(
    plan: &mut AssignmentPlan,
    failed_model_id: Uuid,
    profile: &AssignmentProfile,
    providers: &[ModelProvider],
    models: &[ModelRecord],
    stats: &[ModelStat],
) -> Result<Uuid, AssignmentError> {
    let index = plan
        .decisions
        .iter()
        .position(|decision| decision.chosen_model_id == failed_model_id)
        .ok_or(AssignmentError::InvalidConfiguration)?;
    let role = plan.decisions[index].role.clone();
    let provider_for = |model_id: Uuid| {
        models
            .iter()
            .find(|model| model.id == model_id)
            .map(|model| model.provider_id)
    };
    let mut excluded = BTreeSet::new();
    let mut forbidden_providers = BTreeSet::new();
    for (slot, decision) in plan.decisions.iter().enumerate() {
        if slot == index {
            continue;
        }
        if decision.role == role {
            excluded.insert(decision.chosen_model_id);
            if let Some(provider_id) = provider_for(decision.chosen_model_id) {
                forbidden_providers.insert(provider_id);
            }
        }
        if role == AssignmentRole::Supervisor && decision.role == AssignmentRole::Worker {
            if let Some(provider_id) = provider_for(decision.chosen_model_id) {
                forbidden_providers.insert(provider_id);
            }
        }
        if role == AssignmentRole::Worker && decision.role == AssignmentRole::Supervisor {
            if let Some(provider_id) = provider_for(decision.chosen_model_id) {
                forbidden_providers.insert(provider_id);
            }
        }
    }
    excluded.insert(failed_model_id);
    let decision_cost = plan.decisions[index].estimated_cost_micros;
    let base_total = plan.estimated_cost_micros.saturating_sub(decision_cost);
    let next_candidate = candidates(profile, providers, models, stats, &plan.level, &role)
        .into_iter()
        .find(|candidate| {
            !excluded.contains(&candidate.model.id)
                && !forbidden_providers.contains(&candidate.provider.id)
                && base_total.saturating_add(candidate.estimated_cost)
                    <= profile.per_task_budget_micros
                && base_total.saturating_add(candidate.estimated_cost)
                    <= profile.daily_budget_remaining_micros
        })
        .ok_or(AssignmentError::DiversityUnavailable)?;
    let next = next_candidate.model.id;
    let cost = next_candidate.estimated_cost;
    let decision = &mut plan.decisions[index];
    plan.estimated_cost_micros = base_total.saturating_add(cost);
    decision.chosen_model_id = next;
    decision.estimated_cost_micros = cost;
    decision.reason =
        "Fallback selected after provider failure; role diversity constraints preserved.".into();
    decision
        .filters_applied
        .push("provider_failure_fallback".into());
    Ok(next)
}

#[derive(Debug, Clone, Copy)]
pub struct ModelStatObservation {
    pub completed: bool,
    pub supervisor_passed: bool,
    pub user_retried: bool,
    pub cost_per_run_micros: u64,
    pub latency_ms: u64,
}

/// Update rolling success/retry/cost/latency metrics with a single completed run.
pub fn update_model_stat(
    previous: Option<ModelStat>,
    model_id: Uuid,
    category: &str,
    role: AssignmentRole,
    observation: ModelStatObservation,
) -> ModelStat {
    let now = Utc::now();
    let previous_runs = previous.as_ref().map_or(0, |stat| stat.runs);
    let runs = previous_runs.saturating_add(1);
    let blend = |old: f64, sample: bool| {
        ((old * previous_runs as f64) + if sample { 1.0 } else { 0.0 }) / runs as f64
    };
    ModelStat {
        id: previous.as_ref().map_or_else(Uuid::new_v4, |stat| stat.id),
        model_id,
        category: category.to_owned(),
        role,
        runs,
        completion_rate: blend(
            previous.as_ref().map_or(0.0, |stat| stat.completion_rate),
            observation.completed,
        ),
        supervisor_pass_rate: blend(
            previous
                .as_ref()
                .map_or(0.0, |stat| stat.supervisor_pass_rate),
            observation.supervisor_passed,
        ),
        user_retry_rate: blend(
            previous.as_ref().map_or(0.0, |stat| stat.user_retry_rate),
            observation.user_retried,
        ),
        cost_per_run_micros: previous
            .as_ref()
            .map_or(observation.cost_per_run_micros, |stat| {
                ((u128::from(stat.cost_per_run_micros) * u128::from(previous_runs)
                    + u128::from(observation.cost_per_run_micros))
                    / u128::from(runs))
                .min(u128::from(u64::MAX)) as u64
            }),
        latency_ms: previous.as_ref().map_or(observation.latency_ms, |stat| {
            ((u128::from(stat.latency_ms) * u128::from(previous_runs)
                + u128::from(observation.latency_ms))
                / u128::from(runs))
            .min(u128::from(u64::MAX)) as u64
        }),
        last_updated: now,
    }
}

/// Persist one global registry mutation and its audit event atomically.
pub async fn persist_registry_entity(
    storage: &Arc<dyn Storage>,
    entity: StoredEntity,
    actor_user_id: Uuid,
    event_type: &str,
    entity_type: &str,
    entity_id: &str,
    metadata: serde_json::Value,
) -> Result<(), StorageError> {
    storage
        .apply(StorageMutation {
            entity,
            related_entities: Vec::new(),
            audit: AuditEvent::attributed(
                format!("human:{actor_user_id}"),
                None,
                event_type,
                entity_type,
                entity_id,
                metadata,
            ),
        })
        .await
}

/// Load registry entities from the storage abstraction. Provider/model API
/// credentials are intentionally not represented in registry records.
pub async fn load_registry(
    storage: &Arc<dyn Storage>,
) -> Result<
    (
        Vec<ModelProvider>,
        Vec<ModelRecord>,
        Vec<ModelEvaluation>,
        Vec<ModelStat>,
    ),
    StorageError,
> {
    let providers = storage
        .list_entities(EntityKind::Provider, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Provider(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let models = storage
        .list_entities(EntityKind::Model, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::Model(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let evaluations = storage
        .list_entities(EntityKind::ModelEvaluation, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelEvaluation(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    let stats = storage
        .list_entities(EntityKind::ModelStat, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelStat(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .collect();
    Ok((providers, models, evaluations, stats))
}

/// Record an evaluation and automatically exclude an approved model when its
/// category score drops by at least the configured threshold.
pub async fn record_evaluation(
    storage: &Arc<dyn Storage>,
    evaluation: ModelEvaluation,
    score_drop_threshold: f64,
) -> Result<bool, StorageError> {
    if !evaluation.score.is_finite()
        || !(0.0..=1.0).contains(&evaluation.score)
        || evaluation.test_set_version.trim().is_empty()
        || !score_drop_threshold.is_finite()
        || !(0.0..=1.0).contains(&score_drop_threshold)
    {
        return Err(StorageError::InvalidModelEvaluation);
    }
    let previous: Vec<ModelEvaluation> = storage
        .list_entities(EntityKind::ModelEvaluation, None)
        .await?
        .into_iter()
        .filter_map(|entity| {
            if let StoredEntity::ModelEvaluation(item) = entity {
                Some(item)
            } else {
                None
            }
        })
        .filter(|item| item.model_id == evaluation.model_id && item.category == evaluation.category)
        .collect();
    let latest = previous.iter().max_by_key(|item| item.run_at);
    let dropped = latest.is_some_and(|old| old.score - evaluation.score >= score_drop_threshold);
    let mut related = Vec::new();
    if dropped {
        let mut models: Vec<ModelRecord> = storage
            .list_entities(EntityKind::Model, None)
            .await?
            .into_iter()
            .filter_map(|entity| {
                if let StoredEntity::Model(item) = entity {
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
        if let Some(model) = models
            .iter_mut()
            .find(|item| item.id == evaluation.model_id)
        {
            model.flagged_for_review = true;
            model.healthy = false;
            model.updated_at = Utc::now();
            related.push(StoredEntity::Model(model.clone()));
            related.push(StoredEntity::ModelChangeFlag(ModelChangeFlag {
                id: Uuid::new_v4(),
                model_id: evaluation.model_id,
                change_kind: "score_drop".into(),
                previous_value: latest.map(|item| item.score.to_string()),
                observed_value: Some(evaluation.score.to_string()),
                detected_at: Utc::now(),
                cleared_at: None,
                cleared_by: None,
                resolution_note: None,
            }));
        }
    }
    let event = AuditEvent::attributed(
        "system:model_evaluation",
        None,
        if dropped {
            "model.evaluation_drop_flagged"
        } else {
            "model.evaluation_recorded"
        },
        "model_evaluation",
        evaluation.id.to_string(),
        json!({"model_id": evaluation.model_id, "category": evaluation.category,
            "score": evaluation.score, "test_set_version": evaluation.test_set_version, "score_drop": dropped}),
    );
    storage
        .apply(StorageMutation {
            entity: StoredEntity::ModelEvaluation(evaluation),
            related_entities: related,
            audit: event,
        })
        .await?;
    Ok(dropped)
}

pub fn provider_event_metadata(provider: &ModelProvider) -> serde_json::Value {
    json!({ "provider_id": provider.id, "status": provider.status, "terms_reviewed": provider.terms_reviewed_at.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<ModelProvider>, Vec<ModelRecord>) {
        let provider_a = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider A".into(),
            base_url: "https://a.invalid".into(),
            model_discovery_url: None,
            pricing_source_url: None,
            api_key_env: None,
            discovery_enabled: false,
            rate_limit_per_minute: 10,
            status: "active".into(),
            data_terms: ProviderDataTerms {
                processing_regions: vec!["eu".into()],
                trains_on_api_inputs: Some(false),
                retention_days: Some(0),
                retention_description: "No retention".into(),
            },
            terms_reviewed_at: Some(Utc::now()),
            reviewed_by: Some(Uuid::new_v4()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let provider_b = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider B".into(),
            ..provider_a.clone()
        };
        let provider_c = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider C".into(),
            ..provider_a.clone()
        };
        let provider_d = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider D".into(),
            ..provider_a.clone()
        };
        let provider_e = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider E".into(),
            ..provider_a.clone()
        };
        let provider_f = ModelProvider {
            id: Uuid::new_v4(),
            name: "Provider F".into(),
            ..provider_a.clone()
        };
        let mut abilities = BTreeSet::new();
        abilities.insert("text".into());
        abilities.insert("code".into());
        let mut categories = BTreeSet::new();
        categories.insert("coding".into());
        let model = |provider_id, rank| ModelRecord {
            id: Uuid::new_v4(),
            provider_id,
            model_name: "approved-model".into(),
            status: RegistryStatus::Approved,
            abilities: abilities.clone(),
            categories: categories.clone(),
            context_length: 32_000,
            input_cost_per_million: 1_000_000,
            output_cost_per_million: 1_000_000,
            typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()),
            last_seen_at: Utc::now(),
            notes: String::new(),
            curated_rank: [("coding".to_string(), rank)].into_iter().collect(),
            healthy: true,
            flagged_for_review: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        (
            vec![
                provider_a.clone(),
                provider_b.clone(),
                provider_c.clone(),
                provider_d.clone(),
                provider_e.clone(),
                provider_f.clone(),
            ],
            vec![
                model(provider_a.id, 0),
                model(provider_b.id, 1),
                model(provider_c.id, 2),
                model(provider_d.id, 3),
                model(provider_e.id, 4),
                model(provider_f.id, 5),
            ],
        )
    }

    fn profile(level: TaskLevel) -> AssignmentProfile {
        AssignmentProfile {
            run_id: Uuid::from_u128(5),
            category: "coding".into(),
            level,
            low_risk: false,
            required_abilities: ["text".to_string()].into_iter().collect(),
            required_context_tokens: 8_000,
            privacy: PrivacyRequirement {
                forbid_training: true,
                forbid_retention: true,
                allowed_regions: ["eu".to_string()].into_iter().collect(),
            },
            estimated_input_tokens: 100,
            estimated_output_tokens: 100,
            per_task_budget_micros: 100_000,
            daily_budget_remaining_micros: 100_000,
            exploration_percent: 5,
            minimum_history_runs: 5,
            curated_model_ids: Vec::new(),
        }
    }

    #[test]
    fn rejects_models_missing_required_ability_or_context() {
        let (providers, mut models) = fixture();
        for model in &mut models {
            model.abilities.clear();
            model.context_length = 100;
        }
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn filters_provider_privacy_terms() {
        let (mut providers, models) = fixture();
        providers
            .iter_mut()
            .for_each(|provider| provider.data_terms.trains_on_api_inputs = None);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn important_requires_independent_worker_and_supervisor_providers() {
        let (providers, models) = fixture();
        let plan = assign_models(&profile(TaskLevel::Important), &providers, &models, &[]).unwrap();
        let workers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|item| item.role == AssignmentRole::Worker)
            .collect();
        let supervisors: Vec<_> = plan
            .decisions
            .iter()
            .filter(|item| item.role == AssignmentRole::Supervisor)
            .collect();
        assert_eq!(workers.len(), 2);
        assert_eq!(supervisors.len(), 2);
        assert_ne!(workers[0].chosen_model_id, workers[1].chosen_model_id);
        assert!(supervisors.iter().all(|supervisor| !workers
            .iter()
            .any(|worker| worker.chosen_model_id == supervisor.chosen_model_id)));
    }

    #[test]
    fn cannot_send_unapproved_candidate_to_user_traffic() {
        let (providers, mut models) = fixture();
        models
            .iter_mut()
            .for_each(|model| model.status = RegistryStatus::Candidate);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[test]
    fn important_budget_downgrades_instead_of_exceeding_limit() {
        let (providers, models) = fixture();
        let mut request = profile(TaskLevel::Important);
        request.per_task_budget_micros = 500;
        request.daily_budget_remaining_micros = 500;
        let plan = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(plan.budget_downgraded);
        assert_ne!(plan.level, TaskLevel::Important);
        assert!(plan.estimated_cost_micros <= 500);
    }

    #[test]
    fn history_only_overrides_curated_cold_start_after_minimum_runs() {
        let (providers, models) = fixture();
        let high = ModelStat {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            role: AssignmentRole::Worker,
            runs: 5,
            completion_rate: 1.0,
            supervisor_pass_rate: 1.0,
            user_retry_rate: 0.0,
            cost_per_run_micros: 100,
            latency_ms: 100,
            last_updated: Utc::now(),
        };
        let low_sample = ModelStat {
            runs: 4,
            ..high.clone()
        };
        assert_eq!(
            score_candidate(Some(&low_sample), &TaskLevel::Quick, 5, 2),
            998.0
        );
        assert!(score_candidate(Some(&high), &TaskLevel::Quick, 5, 2) > 998.0);
        assert_eq!(providers.len(), 6);
    }

    #[test]
    fn exploration_is_one_quick_low_risk_assignment_only() {
        let (providers, models) = fixture();
        let mut request = profile(TaskLevel::Quick);
        request.low_risk = true;
        request.run_id = Uuid::from_u128(4);
        let plan = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(plan.exploration_used);
        assert_eq!(
            plan.decisions
                .iter()
                .filter(|decision| decision.role == AssignmentRole::Worker)
                .count(),
            1
        );

        request.level = TaskLevel::Important;
        let important = assign_models(&request, &providers, &models, &[]).unwrap();
        assert!(!important.exploration_used);
    }

    #[test]
    fn assignment_recovery_keeps_worker_supervisor_diversity_after_timeout() {
        let (providers, models) = fixture();
        let request = profile(TaskLevel::Important);
        let mut plan = assign_models(&request, &providers, &models, &[]).unwrap();
        let failed = plan
            .decisions
            .iter()
            .find(|decision| decision.role == AssignmentRole::Worker)
            .unwrap()
            .chosen_model_id;
        let next =
            recover_assignment(&mut plan, failed, &request, &providers, &models, &[]).unwrap();
        assert_ne!(next, failed);
        let worker_providers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|decision| decision.role == AssignmentRole::Worker)
            .filter_map(|decision| {
                models
                    .iter()
                    .find(|model| model.id == decision.chosen_model_id)
                    .map(|model| model.provider_id)
            })
            .collect();
        let supervisor_providers: Vec<_> = plan
            .decisions
            .iter()
            .filter(|decision| decision.role == AssignmentRole::Supervisor)
            .filter_map(|decision| {
                models
                    .iter()
                    .find(|model| model.id == decision.chosen_model_id)
                    .map(|model| model.provider_id)
            })
            .collect();
        assert!(worker_providers
            .iter()
            .all(|provider| !supervisor_providers.contains(provider)));
    }

    #[test]
    fn fallback_chooses_next_approved_provider_after_timeout() {
        let (providers, models) = fixture();
        let mut excluded = BTreeSet::new();
        excluded.insert(models[0].id);
        let mut forbidden = BTreeSet::new();
        forbidden.insert(models[0].provider_id);
        let next = fallback_model(
            &profile(TaskLevel::Quick),
            &providers,
            &models,
            &[],
            &AssignmentRole::Worker,
            &excluded,
            &forbidden,
        );
        assert!(next.is_some_and(|id| id != models[0].id));
    }

    #[test]
    fn model_statistics_update_after_each_run() {
        let model_id = Uuid::new_v4();
        let first = update_model_stat(
            None,
            model_id,
            "coding",
            AssignmentRole::Worker,
            ModelStatObservation {
                completed: true,
                supervisor_passed: true,
                user_retried: false,
                cost_per_run_micros: 100,
                latency_ms: 200,
            },
        );
        let second = update_model_stat(
            Some(first),
            model_id,
            "coding",
            AssignmentRole::Worker,
            ModelStatObservation {
                completed: false,
                supervisor_passed: false,
                user_retried: true,
                cost_per_run_micros: 300,
                latency_ms: 400,
            },
        );
        assert_eq!(second.runs, 2);
        assert_eq!(second.completion_rate, 0.5);
        assert_eq!(second.supervisor_pass_rate, 0.5);
        assert_eq!(second.user_retry_rate, 0.5);
        assert_eq!(second.cost_per_run_micros, 200);
        assert_eq!(second.latency_ms, 300);
    }

    #[test]
    fn flagged_model_is_excluded_from_assignment() {
        let (providers, mut models) = fixture();
        models
            .iter_mut()
            .for_each(|model| model.flagged_for_review = true);
        assert_eq!(
            assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]),
            Err(AssignmentError::NoEligibleModels)
        );
    }

    #[tokio::test]
    async fn evaluation_score_drop_flags_and_excludes_approved_model() {
        use crate::{
            audit::AuditEvent,
            storage::{SqliteStorage, StorageMutation},
        };
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let (providers, models) = fixture();
        for entity in [
            StoredEntity::Provider(providers[0].clone()),
            StoredEntity::Model(models[0].clone()),
        ] {
            storage
                .apply(StorageMutation {
                    entity,
                    related_entities: Vec::new(),
                    audit: AuditEvent::unattributed(
                        None,
                        "test.seeded",
                        "test",
                        "seed",
                        serde_json::Value::Null,
                    ),
                })
                .await
                .unwrap();
        }
        let first = ModelEvaluation {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            test_set_version: "v1".into(),
            score: 0.95,
            run_at: Utc::now(),
            cost_micros: 10,
        };
        assert!(!record_evaluation(&storage, first, 0.10).await.unwrap());
        let second = ModelEvaluation {
            id: Uuid::new_v4(),
            model_id: models[0].id,
            category: "coding".into(),
            test_set_version: "v1".into(),
            score: 0.70,
            run_at: Utc::now() + Duration::seconds(1),
            cost_micros: 12,
        };
        assert!(record_evaluation(&storage, second, 0.10).await.unwrap());
        let stored = storage
            .list_entities(EntityKind::Model, None)
            .await
            .unwrap();
        assert!(
            matches!(&stored[0], StoredEntity::Model(model) if model.flagged_for_review && !model.healthy)
        );
        let flags = storage
            .list_entities(EntityKind::ModelChangeFlag, None)
            .await
            .unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "score_drop")));
    }

    #[test]
    fn diversity_shortfall_degrades_and_is_reported() {
        let (mut providers, mut models) = fixture();
        providers.truncate(1);
        models.truncate(1);
        let plan = assign_models(&profile(TaskLevel::Important), &providers, &models, &[]).unwrap();
        assert_eq!(plan.level, TaskLevel::Quick);
        assert!(plan.diversity_degraded);
        assert!(!plan.notices.is_empty());
    }
}
