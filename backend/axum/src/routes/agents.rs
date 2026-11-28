//! Agents HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials",
            routing::post(handlers::agent_credentials::issue_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id",
            routing::delete(handlers::agent_credentials::revoke_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id/rotate",
            routing::post(handlers::agent_credentials::rotate_agent_credential),
        )
}
