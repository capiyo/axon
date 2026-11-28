//! Tasks HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/tasks",
            routing::get(handlers::tasks::list_tasks).post(handlers::tasks::create_task),
        )
        .route(
            "/api/v1/tasks/:task_id/submit",
            routing::post(handlers::tasks::submit_task),
        )
        .route(
            "/api/v1/tasks/:task_id/execute",
            routing::post(handlers::tasks::execute_task),
        )
        .route(
            "/api/v1/tasks/:task_id/transition",
            routing::post(handlers::tasks::transition_task),
        )
}
