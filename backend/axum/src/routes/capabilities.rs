//! Capabilities HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls",
            routing::post(handlers::capabilities::create_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id",
            routing::get(handlers::capabilities::get_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/nominations",
            routing::post(handlers::capabilities::submit_nomination),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/finalize",
            routing::post(handlers::capabilities::finalize_poll),
        )
}
