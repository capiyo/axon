//! Deterministic vector decision primitives shared by Axon's agent orchestration.
//!
//! Financial indicators are deliberately not imported from Qoin. This module
//! reuses the domain-independent mechanics: normalized feature vectors,
//! task-specific weights, hard constraints before scoring, and outcome feedback.

use crate::model_registry::TaskLevel;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentVector {
    /// Completion, independent supervisor pass, latency efficiency,
    /// cost efficiency, and retry resilience. Each component is in [0, 1].
    pub features: [f64; 5],
    /// Confidence in the observed vector, derived from sample count.
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaskVector {
    /// Importance assigned to the corresponding AgentVector dimensions.
    pub weights: [f64; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAction {
    Execute,
    Delegate,
    Split,
    RequestEvidence,
    Escalate,
    Abstain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionContext {
    pub capability_fit: f64,
    pub evidence_completeness: f64,
    pub ambiguity: f64,
    pub risk: f64,
    pub eligible_alternative_exists: bool,
    pub can_split: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionThresholds {
    pub execute_score: f64,
    pub minimum_evidence: f64,
    pub escalation_risk: f64,
    pub abstain_score: f64,
}

impl Default for DecisionThresholds {
    fn default() -> Self {
        Self {
            execute_score: 0.68,
            minimum_evidence: 0.65,
            escalation_risk: 0.80,
            abstain_score: 0.30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorError {
    NonFiniteValue,
    OutOfRange,
    InvalidWeights,
}

impl AgentVector {
    pub fn from_outcomes(
        completion_rate: f64,
        supervisor_pass_rate: f64,
        latency_ms: u64,
        cost_per_run_micros: u64,
        user_retry_rate: f64,
        runs: u64,
        minimum_history_runs: u64,
    ) -> Result<Self, VectorError> {
        if minimum_history_runs == 0 {
            return Err(VectorError::InvalidWeights);
        }
        let bounded = [
            completion_rate,
            supervisor_pass_rate,
            user_retry_rate,
        ];
        if bounded.iter().any(|value| !value.is_finite()) {
            return Err(VectorError::NonFiniteValue);
        }
        if bounded.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(VectorError::OutOfRange);
        }
        Ok(Self {
            features: [
                completion_rate,
                supervisor_pass_rate,
                1.0 / (1.0 + latency_ms as f64 / 1_000.0),
                1.0 / (1.0 + cost_per_run_micros as f64 / 1_000.0),
                1.0 - user_retry_rate,
            ],
            confidence: runs as f64 / (runs as f64 + minimum_history_runs as f64),
        })
    }
}

impl TaskVector {
    pub fn for_level(level: &TaskLevel) -> Self {
        let weights = match level {
            TaskLevel::Quick => [0.15, 0.10, 0.35, 0.30, 0.10],
            TaskLevel::Normal => [0.30, 0.30, 0.15, 0.15, 0.10],
            TaskLevel::Important => [0.35, 0.40, 0.05, 0.05, 0.15],
        };
        Self { weights }
    }

    pub fn validate(self) -> Result<Self, VectorError> {
        if self.weights.iter().any(|weight| !weight.is_finite()) {
            return Err(VectorError::NonFiniteValue);
        }
        if self.weights.iter().any(|weight| *weight < 0.0) {
            return Err(VectorError::OutOfRange);
        }
        let total: f64 = self.weights.iter().sum();
        if total <= 0.0 || (total - 1.0).abs() > 1e-9 {
            return Err(VectorError::InvalidWeights);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VectorDecisionEngine;

impl VectorDecisionEngine {
    /// Dot-product utility. Cold-start history is blended with the curated
    /// prior according to confidence, so tiny samples cannot dominate.
    pub fn score(
        &self,
        agent: AgentVector,
        task: TaskVector,
        curated_prior: f64,
    ) -> Result<f64, VectorError> {
        let task = task.validate()?;
        if !curated_prior.is_finite() {
            return Err(VectorError::NonFiniteValue);
        }
        if agent.features.iter().any(|value| !value.is_finite())
            || !agent.confidence.is_finite()
            || !(0.0..=1.0).contains(&agent.confidence)
        {
            return Err(VectorError::OutOfRange);
        }
        if agent.features.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(VectorError::OutOfRange);
        }
        let observed: f64 = agent
            .features
            .iter()
            .zip(task.weights.iter())
            .map(|(feature, weight)| feature * weight)
            .sum();
        let prior = curated_prior.clamp(0.0, 1.0);
        Ok(prior * (1.0 - agent.confidence) + observed * agent.confidence)
    }

    /// Chooses an orchestration action after hard eligibility checks have
    /// already been applied. Risk can force escalation regardless of score.
    pub fn choose_action(
        &self,
        context: DecisionContext,
        thresholds: DecisionThresholds,
    ) -> AgentAction {
        if context.risk >= thresholds.escalation_risk {
            return AgentAction::Escalate;
        }
        if context.evidence_completeness < thresholds.minimum_evidence {
            return AgentAction::RequestEvidence;
        }
        if context.capability_fit < thresholds.abstain_score {
            return if context.eligible_alternative_exists {
                AgentAction::Delegate
            } else {
                AgentAction::Abstain
            };
        }
        if context.ambiguity >= 0.75 && context.can_split {
            return AgentAction::Split;
        }
        if context.capability_fit >= thresholds.execute_score {
            AgentAction::Execute
        } else if context.eligible_alternative_exists {
            AgentAction::Delegate
        } else {
            AgentAction::Abstain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_weights_prioritize_speed_and_cost_for_quick_tasks() {
        let quick = TaskVector::for_level(&TaskLevel::Quick);
        let important = TaskVector::for_level(&TaskLevel::Important);
        assert!(quick.weights[2] > important.weights[2]);
        assert!(quick.weights[3] > important.weights[3]);
        assert!(important.weights[1] > quick.weights[1]);
    }

    #[test]
    fn cold_start_prior_dominates_small_samples_and_history_takes_over_gradually() {
        let agent = AgentVector::from_outcomes(1.0, 1.0, 100, 100, 0.0, 1, 10).unwrap();
        let engine = VectorDecisionEngine;
        let score = engine
            .score(agent, TaskVector::for_level(&TaskLevel::Important), 0.4)
            .unwrap();
        assert!(score > 0.4 && score < 0.5);
    }

    #[test]
    fn risk_forces_escalation_even_when_agent_is_capable() {
        let action = VectorDecisionEngine.choose_action(
            DecisionContext {
                capability_fit: 1.0,
                evidence_completeness: 1.0,
                ambiguity: 0.0,
                risk: 0.95,
                eligible_alternative_exists: false,
                can_split: false,
            },
            DecisionThresholds::default(),
        );
        assert_eq!(action, AgentAction::Escalate);
    }

    #[test]
    fn incomplete_evidence_requests_more_evidence_before_execution() {
        let action = VectorDecisionEngine.choose_action(
            DecisionContext {
                capability_fit: 1.0,
                evidence_completeness: 0.2,
                ambiguity: 0.0,
                risk: 0.1,
                eligible_alternative_exists: false,
                can_split: false,
            },
            DecisionThresholds::default(),
        );
        assert_eq!(action, AgentAction::RequestEvidence);
    }

    #[test]
    fn poor_fit_delegates_or_abstains() {
        let engine = VectorDecisionEngine;
        let mut context = DecisionContext {
            capability_fit: 0.1,
            evidence_completeness: 1.0,
            ambiguity: 0.0,
            risk: 0.1,
            eligible_alternative_exists: true,
            can_split: false,
        };
        assert_eq!(
            engine.choose_action(context, DecisionThresholds::default()),
            AgentAction::Delegate
        );
        context.eligible_alternative_exists = false;
        assert_eq!(
            engine.choose_action(context, DecisionThresholds::default()),
            AgentAction::Abstain
        );
    }
}
