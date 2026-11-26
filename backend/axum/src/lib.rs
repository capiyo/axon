pub mod audit;
pub mod auth_middleware;
#[cfg(test)]
mod auth_middleware_tests;
pub mod chat_memory;
pub mod config;
mod chat_memory_routes;
mod memory_document_routes;
mod project_routes;
pub mod firebase_auth;
pub mod identity;
pub mod memory_architecture;
pub mod memory_documents;
pub mod memory_outbox;
pub mod memory_vector_provider;
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
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/retry-sync",
            post(memory_document_routes::retry_memory_document_sync),
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::authenticate,
        ))
        .with_state(state)
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
}
