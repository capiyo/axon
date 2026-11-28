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
        .score(
            agent_vector,
            TaskVector::for_level(match level {
                TaskLevel::Quick => crate::vector_decision::TaskLevel::Quick,
                TaskLevel::Normal => crate::vector_decision::TaskLevel::Normal,
                TaskLevel::Important => crate::vector_decision::TaskLevel::Important,
            }),
            prior,
        )
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
