use crate::auth_middleware::AuthenticatedUser;
use crate::memory_architecture::{
    HandoverRequest, ParetoPolicy, ProgressiveRetrievalRequest, QoinVector,
};
use crate::{
    audit::AuditEvent,
    model::{Artifact, CreateTask, SubmitTask, Task, TaskStatus, TaskTransition, TransitionTask},
    storage::StoredEntity,
    AppState,
};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

type TaskResult<T> = Result<T, (StatusCode, Json<TaskApiError>)>;

#[derive(Debug, Serialize)]
pub struct TaskApiError {
    error: String,
    current_version: Option<u64>,
}

fn storage_failure() -> (StatusCode, Json<TaskApiError>) {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "The change could not be committed. Please retry.",
    )
}

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<TaskApiError>) {
    (
        status,
        Json(TaskApiError {
            error: message.into(),
            current_version: None,
        }),
    )
}

