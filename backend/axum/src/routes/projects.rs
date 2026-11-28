//! Projects HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/members",
            routing::post(handlers::projects::add_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/members/:target_user_id",
            routing::delete(handlers::projects::revoke_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects",
            routing::get(handlers::projects::list_projects).post(handlers::projects::create_project),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members",
            routing::post(handlers::projects::set_project_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members/:target_user_id",
            routing::delete(handlers::projects::revoke_project_member),
        )
}
