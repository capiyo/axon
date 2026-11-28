//! Supervisor HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs",
            routing::get(handlers::supervisor::list_runs).post(handlers::supervisor::create_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id",
            routing::get(handlers::supervisor::get_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checklists",
            routing::post(handlers::supervisor::submit_checklist),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/workers",
            routing::post(handlers::supervisor::submit_worker_result),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checks",
            routing::post(handlers::supervisor::record_checks),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/reviews",
            routing::post(handlers::supervisor::submit_review),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/repair",
            routing::post(handlers::supervisor::start_repair),
        )
}
