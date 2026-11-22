use crate::{
    audit::AuditEvent,
    storage::{EntityKind, Storage, StorageError, StorageMutation, StoredEntity},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryStatus { Candidate, Approved, Disabled }

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
    ChecklistWriter, Worker, Supervisor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLevel { Quick, Normal, Important }

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
    input_cost.saturating_add(output_cost).min(u128::from(u64::MAX)) as u64
}

fn privacy_allows(provider: &ModelProvider, requirement: &PrivacyRequirement) -> bool {
    if requirement.forbid_training && provider.data_terms.trains_on_api_inputs != Some(false) {
        return false;
    }
    if requirement.forbid_retention && provider.data_terms.retention_days != Some(0) {
        return false;
    }
    if !requirement.allowed_regions.is_empty()
        && !provider.data_terms.processing_regions.iter().any(|region| requirement.allowed_regions.contains(region))
    {
        return false;
    }
    true
}

fn score_candidate(
    model: &ModelRecord,
    stat: Option<&ModelStat>,
    level: &TaskLevel,
    minimum_history_runs: u64,
    curated_rank: u16,
) -> f64 {
    let Some(stat) = stat.filter(|item| item.runs >= minimum_history_runs) else {
        return 1000.0 - f64::from(curated_rank);
    };
    let completion = stat.completion_rate.clamp(0.0, 1.0);
    let pass = stat.supervisor_pass_rate.clamp(0.0, 1.0);
    let retries = stat.user_retry_rate.clamp(0.0, 1.0);
    let speed = 1.0 / (1.0 + (stat.latency_ms.max(1) as f64 / 1000.0));
    let economy = 1.0 / (1.0 + stat.cost_per_run_micros as f64 / 1000.0);
    match level {
        TaskLevel::Quick => economy * 40.0 + speed * 35.0 + completion * 15.0 + pass * 10.0 - retries * 10.0,
        TaskLevel::Normal => completion * 30.0 + pass * 30.0 + speed * 20.0 + economy * 20.0 - retries * 15.0,
        TaskLevel::Important => completion * 40.0 + pass * 35.0 + (1.0 - retries) * 15.0 + speed * 5.0 + economy * 5.0,
    }
}

fn candidates<'a>(
    profile: &AssignmentProfile,
    providers: &'a [ModelProvider],
    models: &'a [ModelRecord],
    stats: &'a [ModelStat],
    level: &TaskLevel,
    role: &AssignmentRole,
) -> Vec<Candidate<'a>> {
    let provider_map: BTreeMap<_, _> = providers.iter().map(|provider| (provider.id, provider)).collect();
    let curated: BTreeMap<_, _> = profile.curated_model_ids.iter().enumerate()
        .map(|(index, id)| (*id, index as u16)).collect();
    let mut result = Vec::new();
    for model in models {
        if model.status != RegistryStatus::Approved || !model.healthy || model.flagged_for_review {
            continue;
        }
        if !model.categories.contains(&profile.category)
            || !profile.required_abilities.iter().all(|ability| model.abilities.contains(ability))
            || model.context_length < profile.required_context_tokens
        {
            continue;
        }
        let Some(provider) = provider_map.get(&model.provider_id).copied() else { continue; };
        if provider.status != "active" || !privacy_allows(provider, &profile.privacy) { continue; }
        let stat = stats.iter().find(|item| item.model_id == model.id && item.category == profile.category && item.role == *role);
        let rank = curated.get(&model.id).copied().or_else(|| model.curated_rank.get(&profile.category).copied()).unwrap_or(u16::MAX / 2);
        let score = score_candidate(model, stat, level, profile.minimum_history_runs, rank);
        result.push(Candidate {
            model, provider, stat,
            estimated_cost: estimate_cost(model, profile.estimated_input_tokens, profile.estimated_output_tokens),
            score,
        });
    }
    result.sort_by(|left, right| right.score.total_cmp(&left.score)
        .then_with(|| left.estimated_cost.cmp(&right.estimated_cost))
        .then_with(|| left.model.id.cmp(&right.model.id)));
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
    if bucket >= profile.exploration_percent.min(100) { return None; }
    pool.iter().filter(|candidate| !chosen.contains(&candidate.model.id))
        .filter(|candidate| Utc::now() - candidate.model.last_seen_at < Duration::days(30))
        .max_by(|left, right| left.score.total_cmp(&right.score))
}

pub fn assign_models(
    profile: &AssignmentProfile,
    providers: &[ModelProvider],
    models: &[ModelRecord],
    stats: &[ModelStat],
) -> Result<AssignmentPlan, AssignmentError> {
    if profile.exploration_percent > 100 || profile.minimum_history_runs == 0 {
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
            if pool.is_empty() { failure = Some(AssignmentError::NoEligibleModels); break; }
            for slot in 0..count {
                let chosen = if role == AssignmentRole::Worker {
                    pool.iter().find(|candidate| !worker_models.contains(&candidate.model.id)
                        && !worker_providers.contains(&candidate.provider.id))
                } else if role == AssignmentRole::Supervisor {
                    pool.iter().find(|candidate| !supervisor_models.contains(&candidate.model.id)
                        && !worker_providers.contains(&candidate.provider.id)
                        && !supervisor_providers.contains(&candidate.provider.id))
                } else {
                    pool.iter().find(|candidate| !writer_models.contains(&candidate.model.id)
                        && !writer_providers.contains(&candidate.provider.id))
                        .or_else(|| pool.iter().find(|candidate| !writer_models.contains(&candidate.model.id)))
                };
                let Some(chosen) = chosen else {
                    if role == AssignmentRole::ChecklistWriter { diversity_degraded = true; continue; }
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
                    id: Uuid::new_v4(), run_id: profile.run_id, role: role.clone(), slot: slot as u8,
                    candidates_considered: considered,
                    filters_applied: vec![
                        "approved_only".into(), "healthy_only".into(), "category".into(),
                        "abilities".into(), "context_length".into(), "privacy_terms".into(),
                        "provider_active".into(),
                    ],
                    chosen_model_id: chosen.model.id,
                    reason: if chosen.stat.is_some_and(|stat| stat.runs >= profile.minimum_history_runs) {
                        "Selected by category performance, cost and latency history.".into()
                    } else { "Selected by the configured curated cold-start ranking.".into() },
                    estimated_cost_micros: cost,
                    created_at: Utc::now(),
                });
            }
            if failure.is_some() { break; }
        }

        if failure.is_none() {
            let mut exploration_used = false;
            if level == TaskLevel::Quick && profile.low_risk {
                let pool = candidates(profile, providers, models, stats, &level, &AssignmentRole::Worker);
                if let Some(exploration) = exploration_candidate(&pool, &worker_models, profile, &level) {
                    if let Some(decision) = decisions.iter_mut().find(|decision| decision.role == AssignmentRole::Worker) {
                        let revised_total = estimated_total
                            .saturating_sub(decision.estimated_cost_micros)
                            .saturating_add(exploration.estimated_cost);
                        if revised_total <= profile.per_task_budget_micros
                            && revised_total <= profile.daily_budget_remaining_micros {
                            decision.candidates_considered = pool.iter().map(|candidate| candidate.model.id).collect();
                            decision.filters_applied.push("low_risk_quick_exploration".into());
                            decision.chosen_model_id = exploration.model.id;
                            decision.reason = "Approved recently-seen model selected by configured low-risk exploration.".into();
                            decision.estimated_cost_micros = exploration.estimated_cost;
                            estimated_total = revised_total;
                            exploration_used = true;
                        }
                    }
                }
            }
            if budget_downgraded { notices.push("The requested supervision level exceeded the configured budget; Axon selected a lower-cost level.".into()); }
            if diversity_degraded { notices.push("Checklist writers could not be assigned distinct providers; writer diversity was reduced.".into()); }
            return Ok(AssignmentPlan { level, decisions, estimated_cost_micros: estimated_total,
                budget_downgraded, exploration_used, diversity_degraded, notices });
        }

        match failure.expect("failure checked") {
            AssignmentError::BudgetExceeded if level != TaskLevel::Quick => {
                level = match level { TaskLevel::Important => TaskLevel::Normal, TaskLevel::Normal => TaskLevel::Quick, TaskLevel::Quick => unreachable!() };
                budget_downgraded = true;
            }
            AssignmentError::DiversityUnavailable if level != TaskLevel::Quick => {
                level = match level { TaskLevel::Important => TaskLevel::Normal, TaskLevel::Normal => TaskLevel::Quick, TaskLevel::Quick => unreachable!() };
                diversity_degraded = true;
                notices.push("Provider diversity could not be satisfied; Axon reduced the supervision level.".into());
            }
            other => return Err(other),
        }
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
    storage.apply(StorageMutation {
        entity,
        related_entities: Vec::new(),
        audit: AuditEvent::attributed(
            format!("human:{actor_user_id}"), None, event_type, entity_type, entity_id, metadata,
        ),
    }).await
}

/// Load registry entities from the storage abstraction. Provider/model API
/// credentials are intentionally not represented in registry records.
pub async fn load_registry(
    storage: &Arc<dyn Storage>,
) -> Result<(Vec<ModelProvider>, Vec<ModelRecord>, Vec<ModelEvaluation>, Vec<ModelStat>), StorageError> {
    let providers = storage.list_entities(EntityKind::Provider, None).await?.into_iter().filter_map(|entity| {
        if let StoredEntity::Provider(item) = entity { Some(item) } else { None }
    }).collect();
    let models = storage.list_entities(EntityKind::Model, None).await?.into_iter().filter_map(|entity| {
        if let StoredEntity::Model(item) = entity { Some(item) } else { None }
    }).collect();
    let evaluations = storage.list_entities(EntityKind::ModelEvaluation, None).await?.into_iter().filter_map(|entity| {
        if let StoredEntity::ModelEvaluation(item) = entity { Some(item) } else { None }
    }).collect();
    let stats = storage.list_entities(EntityKind::ModelStat, None).await?.into_iter().filter_map(|entity| {
        if let StoredEntity::ModelStat(item) = entity { Some(item) } else { None }
    }).collect();
    Ok((providers, models, evaluations, stats))
}

pub fn provider_event_metadata(provider: &ModelProvider) -> serde_json::Value {
    json!({ "provider_id": provider.id, "status": provider.status, "terms_reviewed": provider.terms_reviewed_at.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<ModelProvider>, Vec<ModelRecord>) {
        let provider_a = ModelProvider {
            id: Uuid::new_v4(), name: "Provider A".into(), base_url: "https://a.invalid".into(),
            status: "active".into(),
            data_terms: ProviderDataTerms { processing_regions: vec!["eu".into()], trains_on_api_inputs: Some(false), retention_days: Some(0), retention_description: "No retention".into() },
            terms_reviewed_at: Some(Utc::now()), reviewed_by: Some(Uuid::new_v4()), created_at: Utc::now(), updated_at: Utc::now(),
        };
        let provider_b = ModelProvider { id: Uuid::new_v4(), name: "Provider B".into(), ..provider_a.clone() };
        let provider_c = ModelProvider { id: Uuid::new_v4(), name: "Provider C".into(), ..provider_a.clone() };
        let provider_d = ModelProvider { id: Uuid::new_v4(), name: "Provider D".into(), ..provider_a.clone() };
        let mut abilities = BTreeSet::new(); abilities.insert("text".into()); abilities.insert("code".into());
        let mut categories = BTreeSet::new(); categories.insert("coding".into());
        let model = |provider_id| ModelRecord {
            id: Uuid::new_v4(), provider_id, model_name: "approved-model".into(), status: RegistryStatus::Approved,
            abilities: abilities.clone(), categories: categories.clone(), context_length: 32_000,
            input_cost_per_million: 1_000_000, output_cost_per_million: 1_000_000, typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(), version: Some("v1".into()), last_seen_at: Utc::now(),
            notes: String::new(), curated_rank: BTreeMap::new(), healthy: true, flagged_for_review: false,
            created_at: Utc::now(), updated_at: Utc::now(),
        };
        (vec![provider_a.clone(), provider_b.clone(), provider_c.clone(), provider_d.clone()],
         vec![model(provider_a.id), model(provider_b.id), model(provider_c.id), model(provider_d.id)])
    }

    fn profile(level: TaskLevel) -> AssignmentProfile {
        AssignmentProfile {
            run_id: Uuid::from_u128(5), category: "coding".into(), level, low_risk: false,
            required_abilities: ["text".to_string()].into_iter().collect(), required_context_tokens: 8_000,
            privacy: PrivacyRequirement { forbid_training: true, forbid_retention: true, allowed_regions: ["eu".to_string()].into_iter().collect() },
            estimated_input_tokens: 100, estimated_output_tokens: 100, per_task_budget_micros: 100_000,
            daily_budget_remaining_micros: 100_000, exploration_percent: 5, minimum_history_runs: 5,
            curated_model_ids: Vec::new(),
        }
    }

    #[test]
    fn rejects_models_missing_required_ability_or_context() {
        let (providers, mut models) = fixture();
        models[0].abilities.clear();
        models[1].context_length = 100;
        assert_eq!(assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]), Err(AssignmentError::NoEligibleModels));
    }

    #[test]
    fn filters_provider_privacy_terms() {
        let (mut providers, models) = fixture();
        providers.iter_mut().for_each(|provider| provider.data_terms.trains_on_api_inputs = None);
        assert_eq!(assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]), Err(AssignmentError::NoEligibleModels));
    }

    #[test]
    fn important_requires_independent_worker_and_supervisor_providers() {
        let (providers, models) = fixture();
        let plan = assign_models(&profile(TaskLevel::Important), &providers, &models, &[]).unwrap();
        let workers: Vec<_> = plan.decisions.iter().filter(|item| item.role == AssignmentRole::Worker).collect();
        let supervisors: Vec<_> = plan.decisions.iter().filter(|item| item.role == AssignmentRole::Supervisor).collect();
        assert_eq!(workers.len(), 2);
        assert_eq!(supervisors.len(), 2);
        assert_ne!(workers[0].chosen_model_id, workers[1].chosen_model_id);
        assert!(supervisors.iter().all(|supervisor| !workers.iter().any(|worker| worker.chosen_model_id == supervisor.chosen_model_id)));
    }

    #[test]
    fn cannot_send_unapproved_candidate_to_user_traffic() {
        let (providers, mut models) = fixture();
        models.iter_mut().for_each(|model| model.status = RegistryStatus::Candidate);
        assert_eq!(assign_models(&profile(TaskLevel::Quick), &providers, &models, &[]), Err(AssignmentError::NoEligibleModels));
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
