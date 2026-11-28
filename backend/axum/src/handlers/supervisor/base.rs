use crate::auth_middleware::AuthenticatedUser;
use crate::{
    audit::AuditEvent,
    model_registry::{
        AssignmentProfile, ModelProvider, ModelRecord, ModelStat, PrivacyRequirement, TaskLevel,
    },
    storage::{EntityKind, StoredEntity},
    supervisor::{
        ChecklistProposal, DeterministicCheck, SupervisorError, SupervisorReview, SupervisorRun,
        SupervisorRunStatus, WorkerResult,
    },
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;

#[derive(Debug, Serialize)]
pub struct ApiError {
    error: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSupervisorRun {
    pub title: String,
    pub objective: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_level")]
    pub level: TaskLevel,
    #[serde(default)]
    pub required_abilities: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub required_context_tokens: u64,
    #[serde(default)]
    pub privacy: Option<PrivacyRequirement>,
}
fn default_category() -> String {
    "general".into()
}
fn default_level() -> TaskLevel {
    TaskLevel::Normal
}

fn is_low_risk_quick(level: &TaskLevel, title: &str, objective: &str) -> bool {
    if *level != TaskLevel::Quick || objective.len() > 4_000 {
        return false;
    }
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let high_risk_terms = [
        "delete",
        "drop table",
        "truncate",
        "migration",
        "deploy",
        "production",
        "security",
        "authentication",
        "authorization",
        "password",
        "secret",
        "payment",
        "financial",
        "medical",
        "legal",
        "execute command",
        "shell",
        "database",
        "credential",
        "account access",
    ];
    !high_risk_terms.iter().any(|term| text.contains(term))
}

fn task_risk_score(title: &str, objective: &str) -> f64 {
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let high_risk_terms = [
        "delete",
        "drop table",
        "truncate",
        "migration",
        "deploy",
        "production",
        "security",
        "authentication",
        "authorization",
        "password",
        "secret",
        "payment",
        "financial",
        "medical",
        "legal",
        "execute command",
        "shell",
        "database",
        "credential",
        "account access",
    ];
    if high_risk_terms.iter().any(|term| text.contains(term)) {
        0.95
    } else {
        0.25
    }
}

fn task_ambiguity(title: &str, objective: &str) -> f64 {
    let text = format!("{title} {objective}").to_ascii_lowercase();
    let vague_terms = [
        "somehow",
        "maybe",
        "some stuff",
        "etc",
        "as needed",
        "make it better",
        "figure it out",
    ];
    let vague_count = vague_terms
        .iter()
        .filter(|term| text.contains(**term))
        .count();
    if vague_count >= 2 || objective.len() > 8_000 {
        0.85
    } else if vague_count == 1 || objective.len() > 4_000 {
        0.65
    } else {
        0.20
    }
}

fn configured_curated_models(category: &str) -> Vec<Uuid> {
    let suffix: String = category
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let key = format!("AXON_CURATED_MODELS_{suffix}");
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| Uuid::parse_str(value.trim()).ok())
        .take(6)
        .collect()
}

async fn persist_created_run(
    state: &AppState,
    user: &AuthenticatedUser,
    run: SupervisorRun,
) -> ApiResult<SupervisorRun> {
    let related_entities = run
        .assignment_decisions
        .iter()
        .cloned()
        .map(StoredEntity::AssignmentDecision)
        .collect();
    state
        .persist_entity(
            StoredEntity::SupervisorRun(run.clone()),
            related_entities,
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(run.workspace_id),
                "supervisor_run.created",
                "supervisor_run",
                run.id.to_string(),
                json!({
                    "status": run.status, "assignment_level": run.assignment_level,
                    "assignment_decisions": run.assignment_decisions.len(),
                    "assignment_notice": run.assignment_notice
                }),
            ),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Supervisor run could not be persisted.",
            )
        })?;
    state
        .supervisor_runs
        .write()
        .await
        .insert(run.id, run.clone());
    Ok(run)
}

#[derive(Debug, Deserialize)]
pub struct RecordChecks {
    pub checks: Vec<DeterministicCheck>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

fn supervisor_error(error: SupervisorError) -> (StatusCode, Json<ApiError>) {
    let status = match &error {
        SupervisorError::InvalidRunState
        | SupervisorError::DuplicateSubmission
        | SupervisorError::RepairNotAvailable => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    api_error(status, error.to_string())
}

async fn persist_run(
    state: &AppState,
    user: &AuthenticatedUser,
    run: SupervisorRun,
    event_type: &'static str,
) -> ApiResult<SupervisorRun> {
    state
        .persist_entity(
            StoredEntity::SupervisorRun(run.clone()),
            Vec::new(),
            AuditEvent::attributed(
                format!("human:{}", user.user_id),
                Some(run.workspace_id),
                event_type,
                "supervisor_run",
                run.id.to_string(),
                json!({
                    "status": run.status,
                    "version": run.version,
                    "repair_rounds_used": run.repair_rounds_used
                }),
            ),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Supervisor run could not be persisted.",
            )
        })?;
    state
        .supervisor_runs
        .write()
        .await
        .insert(run.id, run.clone());
    Ok(run)
}

async fn find_run(
    state: &AppState,
    workspace_id: Uuid,
    run_id: Uuid,
) -> Result<SupervisorRun, (StatusCode, Json<ApiError>)> {
    let runs = state.supervisor_runs.read().await;
    runs.get(&run_id)
        .filter(|run| run.workspace_id == workspace_id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Supervisor run not found."))
}

