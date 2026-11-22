use crate::{
    ai_provider::AiProviderError,
    supervisor::{ChecklistProposal, SupervisorReview, WorkerResult},
};
use async_trait::async_trait;

/// Shared, provider-neutral task context supplied to a supervisor role.
#[derive(Debug, Clone)]
pub struct RoleTaskContext {
    pub run_id: String,
    pub category: String,
    pub title: String,
    pub objective: String,
    pub context: String,
}

/// A worker receives the merged checklist and returns a structured result.
/// Artifact persistence is owned by the supervisor orchestration layer.
#[derive(Debug, Clone)]
pub struct WorkerTaskContext {
    pub task: RoleTaskContext,
    pub merged_checklist_json: String,
    pub repair_instructions: Vec<String>,
}

/// A supervisor receives worker outputs and deterministic check evidence.
#[derive(Debug, Clone)]
pub struct ReviewTaskContext {
    pub task: RoleTaskContext,
    pub worker_results_json: String,
    pub deterministic_checks_json: String,
}

#[async_trait]
pub trait ChecklistWriter: Send + Sync {
    async fn write_checklist(
        &self,
        task: RoleTaskContext,
    ) -> Result<ChecklistProposal, AiProviderError>;
}

#[async_trait]
pub trait Worker: Send + Sync {
    async fn execute(&self, task: WorkerTaskContext) -> Result<WorkerResult, AiProviderError>;
}

#[async_trait]
pub trait Supervisor: Send + Sync {
    async fn review(&self, task: ReviewTaskContext) -> Result<SupervisorReview, AiProviderError>;
}
