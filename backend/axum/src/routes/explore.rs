//! Explore HTTP endpoints.
use crate::{explore, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/explore/agents", routing::get(explore::list_agents))
        .route("/api/v1/explore/agents/:listing_id", routing::get(explore::get_agent))
        .route("/api/v1/workspaces/:workspace_id/agents/:agent_id/publish", routing::post(explore::publish_agent).routing::delete(explore::unpublish_agent))
        .route("/api/v1/explore/agents/:listing_id/comments", routing::get(explore::list_comments).routing::post(explore::create_comment))
        .route("/api/v1/explore/comments/:comment_id", routing::patch(explore::edit_comment).routing::delete(explore::delete_comment))
        .route("/api/v1/explore/agents/:listing_id/like", routing::post(explore::like_listing))
        .route("/api/v1/explore/agents/:listing_id/follow", routing::post(explore::follow_listing))
        .route("/api/v1/explore/comments/:comment_id/like", routing::post(explore::like_comment))
        .route("/api/v1/explore/reports", routing::post(explore::report_content))
        .route("/api/v1/explore/agents/:listing_id/reports", routing::get(explore::list_reports))
        .route("/api/v1/explore/reports/:report_id/resolve", routing::post(explore::resolve_report))
        .route("/api/v1/notifications", routing::get(explore::list_notifications))
        .route("/api/v1/notifications/read-all", routing::post(explore::mark_all_notifications_read))
        .route("/api/v1/notifications/:notification_id/read", routing::post(explore::mark_notification_read))
}
