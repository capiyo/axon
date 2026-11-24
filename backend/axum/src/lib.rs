pub mod agent_runtime;
pub mod audit;
pub mod auth_middleware;
#[cfg(test)]
mod auth_middleware_tests;
pub mod chat_memory;
mod chat_memory_routes;
pub mod explore;
pub mod firebase_auth;
pub mod identity;
pub mod memory_architecture;
pub mod memory_intelligence;
mod memory_routes;
mod model;
pub mod qoin_vectors;
pub mod robotics;
mod robotics_routes;
mod routes;
mod state;
pub mod storage;
mod task_routes;

pub use state::AppState;

use axum::{
    http::{header, HeaderValue, Method},
    middleware,
    routing::{get, patch, post},
    Router,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    trace::TraceLayer,
};

pub fn app(state: AppState) -> Router {
    let allowed_origins = std::env::var("AXON_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|origin| HeaderValue::from_str(origin.trim()).ok())
        .collect::<Vec<_>>();
    let cors = if allowed_origins.is_empty() {
        CorsLayer::new()
    } else {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(allowed_origins))
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    };
    Router::new()
        .route("/health", get(routes::health))
        .route("/api/v1/explore/agents", get(explore::list_agents))
        .route("/api/v1/explore/agents/:listing_id", get(explore::get_agent))
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/publish",
            post(explore::publish_agent).delete(explore::unpublish_agent),
        )
        .route(
            "/api/v1/explore/agents/:listing_id/comments",
            get(explore::list_comments).post(explore::create_comment),
        )
        .route(
            "/api/v1/explore/comments/:comment_id",
            patch(explore::edit_comment).delete(explore::delete_comment),
        )
        .route(
            "/api/v1/explore/agents/:listing_id/like",
            post(explore::like_listing),
        )
        .route(
            "/api/v1/explore/agents/:listing_id/follow",
            post(explore::follow_listing),
        )
        .route(
            "/api/v1/explore/comments/:comment_id/like",
            post(explore::like_comment),
        )
        .route("/api/v1/explore/reports", post(explore::report_content))
        .route(
            "/api/v1/explore/agents/:listing_id/reports",
            get(explore::list_reports),
        )
        .route(
            "/api/v1/explore/reports/:report_id/resolve",
            post(explore::resolve_report),
        )
        .route("/api/v1/notifications", get(explore::list_notifications))
        .route("/api/v1/notifications/read-all", post(explore::mark_all_notifications_read))
        .route("/api/v1/notifications/:notification_id/read", post(explore::mark_notification_read))
        .route(
            "/api/v1/workspaces/:workspace_id/chats",
            get(chat_memory_routes::list_chats).post(chat_memory_routes::create_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id",
            get(chat_memory_routes::get_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages",
            post(chat_memory_routes::append_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/archive",
            patch(chat_memory_routes::archive_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/read",
            post(chat_memory_routes::mark_chat_read),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/like",
            post(chat_memory_routes::toggle_chat_like),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory",
            post(chat_memory_routes::create_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory/:entry_id/:action",
            post(chat_memory_routes::mutate_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/index",
            post(memory_routes::index_memory),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/memory-index",
            post(memory_routes::index_chat_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/epoch",
            get(memory_routes::latest_epoch),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/qoin-vector",
            get(memory_routes::latest_qoin_vector),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory-index/:entry_id/qoin",
            post(memory_routes::index_memory_with_qoin),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/qoin-index",
            post(memory_routes::index_chat_message_with_qoin),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-context",
            post(memory_routes::prepare_handover),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/retrieve",
            post(memory_routes::progressive_retrieval),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/audit",
            get(routes::list_audit_events),
        )
        .route(
            "/api/v1/sim/devices/:device_id/telemetry",
            get(robotics_routes::list_simulated_telemetry)
                .post(robotics_routes::ingest_simulated_telemetry),
        )
        .route(
            "/api/v1/workspaces",
            get(routes::list_workspaces).post(routes::create_workspace),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents",
            get(routes::list_agents).post(routes::register_agent),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/artifacts",
            get(routes::list_artifacts).post(routes::create_artifact),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/tasks",
            get(task_routes::list_tasks).post(task_routes::create_task),
        )
        .route(
            "/api/v1/tasks/:task_id/execute",
            post(task_routes::execute_task),
        )
        .route(
            "/api/v1/tasks/:task_id/submit",
            post(task_routes::submit_task),
        )
        .route(
            "/api/v1/tasks/:task_id/transitions",
            post(task_routes::transition_task),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::authenticate,
        ))
        .with_state(state)
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
}
