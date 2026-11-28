//! System HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/health", routing::get(handlers::system::health))
        .route("/ready", routing::get(handlers::system::readiness))
        .route(
            "/api/v1/workspaces/:workspace_id/audit",
            routing::get(handlers::system::list_audit_events),
        )
        .route(
            "/api/v1/workspaces",
            routing::get(handlers::system::list_workspaces)
                .post(handlers::system::create_workspace),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents",
            routing::get(handlers::system::list_agents)
                .post(handlers::system::register_agent),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/artifacts",
            routing::get(handlers::system::list_artifacts)
                .post(handlers::system::create_artifact),
        )
}
