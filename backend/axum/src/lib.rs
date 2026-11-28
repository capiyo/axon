mod agent_credential_routes;
mod agent_memory_routes;
pub mod agent_runtime;
pub mod ai_provider;
pub mod audit;
pub mod auth_middleware;
#[cfg(test)]
mod auth_middleware_tests;
pub mod chat_memory;
pub mod capability;
mod capability_routes;
mod chat_memory_routes;
pub mod config;
pub mod explore;
pub mod firebase_auth;
pub mod identity;
pub mod memory_architecture;
pub mod memory;
mod memory_document_routes;
pub mod memory_documents;
pub mod memory_store;
pub mod memory_system;
pub mod memory_intelligence;
pub mod memory_outbox;
mod memory_routes;
pub mod memory_vector_provider;
mod model;
pub mod model_registry;
pub mod model_discovery;
mod model_registry_routes;
mod project_routes;
pub mod qoin_vectors;
pub mod robotics;
mod robotics_routes;
mod routes;
mod state;
pub mod storage;
pub mod supervisor;
mod supervisor_routes;
pub mod supervisor_roles;
mod task_routes;
pub mod vector_decision;

pub use state::AppState;

use axum::{
    http::{header, HeaderValue, Method},
    middleware,
    routing::{delete, get, patch, post},
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
        .route(
            "/api/v1/admin/providers",
            get(model_registry_routes::list_providers).post(model_registry_routes::create_provider),
        )
        .route(
            "/api/v1/admin/providers/:provider_id/terms-review",
            post(model_registry_routes::review_provider_terms),
        )
        .route(
            "/api/v1/admin/models",
            get(model_registry_routes::list_models).post(model_registry_routes::create_model),
        )
        .route(
            "/api/v1/admin/models/:model_id/review",
            post(model_registry_routes::review_candidate),
        )
        .route("/api/v1/admin/evaluations", get(model_registry_routes::list_evaluations))
        .route("/api/v1/admin/stats", get(model_registry_routes::list_stats))
        .route("/api/v1/admin/flags", get(model_registry_routes::list_flags))
        .route("/api/v1/admin/flags/:flag_id/clear", post(model_registry_routes::clear_flag))
        .route("/api/v1/admin/assignments", get(model_registry_routes::list_assignments))
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls",
            post(capability_routes::create_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id",
            get(capability_routes::get_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/nominations",
            post(capability_routes::submit_nomination),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/finalize",
            post(capability_routes::finalize_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs",
            get(supervisor_routes::list_runs).post(supervisor_routes::create_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id",
            get(supervisor_routes::get_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checklists",
            post(supervisor_routes::submit_checklist),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/workers",
            post(supervisor_routes::submit_worker_result),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checks",
            post(supervisor_routes::record_checks),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/reviews",
            post(supervisor_routes::submit_review),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/repair",
            post(supervisor_routes::start_repair),
        )
        .route("/api/v1/explore/agents", get(explore::list_agents))
        .route("/api/v1/explore/agents/:listing_id", get(explore::get_agent))
        .route("/api/v1/workspaces/:workspace_id/agents/:agent_id/publish", post(explore::publish_agent).delete(explore::unpublish_agent))
        .route("/api/v1/explore/agents/:listing_id/comments", get(explore::list_comments).post(explore::create_comment))
        .route("/api/v1/explore/comments/:comment_id", patch(explore::edit_comment).delete(explore::delete_comment))
        .route("/api/v1/explore/agents/:listing_id/like", post(explore::like_listing))
        .route("/api/v1/explore/agents/:listing_id/follow", post(explore::follow_listing))
        .route("/api/v1/explore/comments/:comment_id/like", post(explore::like_comment))
        .route("/api/v1/explore/reports", post(explore::report_content))
        .route("/api/v1/explore/agents/:listing_id/reports", get(explore::list_reports))
        .route("/api/v1/explore/reports/:report_id/resolve", post(explore::resolve_report))
        .route("/api/v1/notifications", get(explore::list_notifications))
        .route("/api/v1/notifications/read-all", post(explore::mark_all_notifications_read))
        .route("/api/v1/notifications/:notification_id/read", post(explore::mark_notification_read))
        .route("/ready", get(routes::readiness))
        .route(
            "/api/v1/workspaces/:workspace_id/chats",
            get(chat_memory_routes::list_chats).post(chat_memory_routes::create_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id",
            get(chat_memory_routes::get_chat),
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
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages",
            post(chat_memory_routes::append_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/archive",
            patch(chat_memory_routes::archive_chat),
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
            "/api/v1/workspaces/:workspace_id/agent-memories",
            post(agent_memory_routes::propose),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/allocate",
            post(agent_memory_routes::allocate),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/review",
            post(agent_memory_routes::review),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/share",
            post(agent_memory_routes::share),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/share/:agent_id",
            delete(agent_memory_routes::revoke),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents",
            post(memory_document_routes::create_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/reconciliation",
            get(memory_document_routes::reconcile_memory_documents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/approve",
            post(memory_document_routes::approve_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/archive",
            post(memory_document_routes::archive_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/restore",
            post(memory_document_routes::restore_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version",
            delete(memory_document_routes::delete_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/retry-sync",
            post(memory_document_routes::retry_memory_document_sync),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials",
            post(agent_credential_routes::issue_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id",
            delete(agent_credential_routes::revoke_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id/rotate",
            post(agent_credential_routes::rotate_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/:memory_id/:version",
            get(memory_document_routes::get_agent_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/:memory_id/versions/history",
            get(memory_document_routes::get_agent_memory_document_versions),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/search",
            post(memory_document_routes::search_agent_memory_documents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/documents",
            get(memory_document_routes::get_agent_session_documents),
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
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/context",
            post(memory_routes::retrieve_session_context),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/feedback",
            post(memory_routes::record_session_feedback),
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
            "/api/v1/workspaces/:workspace_id/members",
            post(project_routes::add_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/members/:target_user_id",
            delete(project_routes::revoke_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects",
            get(project_routes::list_projects).post(project_routes::create_project),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members",
            post(project_routes::set_project_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members/:target_user_id",
            delete(project_routes::revoke_project_member),
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
            "/api/v1/tasks/:task_id/submit",
            post(task_routes::submit_task),
        )
        .route(
            "/api/v1/tasks/:task_id/execute",
            post(task_routes::execute_task),
        )
        .route(
            "/api/v1/tasks/:task_id/transition",
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
