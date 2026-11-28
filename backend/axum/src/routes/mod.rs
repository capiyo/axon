use crate::{handlers, AppState, auth_middleware};
use axum::{http::{header, HeaderValue, Method}, middleware, routing::{delete, get, patch, post}, Router};
use tower_http::{cors::{AllowOrigin, CorsLayer}, limit::RequestBodyLimitLayer, trace::TraceLayer};

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
        .route("/health", get(handlers::system::health))
        .route(
            "/api/v1/admin/providers",
            get(handlers::model_registry::list_providers).post(handlers::model_registry::create_provider),
        )
        .route(
            "/api/v1/admin/providers/:provider_id/terms-review",
            post(handlers::model_registry::review_provider_terms),
        )
        .route(
            "/api/v1/admin/models",
            get(handlers::model_registry::list_models).post(handlers::model_registry::create_model),
        )
        .route(
            "/api/v1/admin/models/:model_id/review",
            post(handlers::model_registry::review_candidate),
        )
        .route("/api/v1/admin/evaluations", get(handlers::model_registry::list_evaluations))
        .route("/api/v1/admin/stats", get(handlers::model_registry::list_stats))
        .route("/api/v1/admin/flags", get(handlers::model_registry::list_flags))
        .route("/api/v1/admin/flags/:flag_id/clear", post(handlers::model_registry::clear_flag))
        .route("/api/v1/admin/assignments", get(handlers::model_registry::list_assignments))
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls",
            post(handlers::capabilities::create_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id",
            get(handlers::capabilities::get_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/nominations",
            post(handlers::capabilities::submit_nomination),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/capability-polls/:poll_id/finalize",
            post(handlers::capabilities::finalize_poll),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs",
            get(handlers::supervisor::list_runs).post(handlers::supervisor::create_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id",
            get(handlers::supervisor::get_run),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checklists",
            post(handlers::supervisor::submit_checklist),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/workers",
            post(handlers::supervisor::submit_worker_result),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/checks",
            post(handlers::supervisor::record_checks),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/reviews",
            post(handlers::supervisor::submit_review),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/supervisor-runs/:run_id/repair",
            post(handlers::supervisor::start_repair),
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
        .route("/ready", get(handlers::system::readiness))
        .route(
            "/api/v1/workspaces/:workspace_id/chats",
            get(handlers::chats::list_chats).post(handlers::chats::create_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id",
            get(handlers::chats::get_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/read",
            post(handlers::chats::mark_chat_read),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/like",
            post(handlers::chats::toggle_chat_like),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages",
            post(handlers::chats::append_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/archive",
            patch(handlers::chats::archive_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory",
            post(handlers::chats::create_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory/:entry_id/:action",
            post(handlers::chats::mutate_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/imports",
            post(handlers::memory_imports::import_conversation),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/imports/:import_id",
            get(handlers::memory_imports::get_import),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memories",
            get(handlers::memory_imports::list_memories).post(handlers::memory_imports::create_memory),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/context",
            get(handlers::memory_imports::memory_context),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memories/:memory_id/review",
            post(handlers::memory_imports::review_memory),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories",
            post(handlers::agent_memory::propose),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/allocate",
            post(handlers::agent_memory::allocate),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/review",
            post(handlers::agent_memory::review),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/share",
            post(handlers::agent_memory::share),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agent-memories/:memory_id/share/:agent_id",
            delete(handlers::agent_memory::revoke),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents",
            post(handlers::memory_documents::create_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/reconciliation",
            get(handlers::memory_documents::reconcile_memory_documents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/approve",
            post(handlers::memory_documents::approve_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/archive",
            post(handlers::memory_documents::archive_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/restore",
            post(handlers::memory_documents::restore_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version",
            delete(handlers::memory_documents::delete_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/documents/:memory_id/:version/retry-sync",
            post(handlers::memory_documents::retry_memory_document_sync),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials",
            post(handlers::agent_credentials::issue_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id",
            delete(handlers::agent_credentials::revoke_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/credentials/:credential_id/rotate",
            post(handlers::agent_credentials::rotate_agent_credential),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/:memory_id/:version",
            get(handlers::memory_documents::get_agent_memory_document),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/:memory_id/versions/history",
            get(handlers::memory_documents::get_agent_memory_document_versions),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-documents/search",
            post(handlers::memory_documents::search_agent_memory_documents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/documents",
            get(handlers::memory_documents::get_agent_session_documents),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/index",
            post(handlers::memory::index_memory),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/memory-index",
            post(handlers::memory::index_chat_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/epoch",
            get(handlers::memory::latest_epoch),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/memory/qoin-vector",
            get(handlers::memory::latest_qoin_vector),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory-index/:entry_id/qoin",
            post(handlers::memory::index_memory_with_qoin),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/qoin-index",
            post(handlers::memory::index_chat_message_with_qoin),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-context",
            post(handlers::memory::prepare_handover),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/retrieve",
            post(handlers::memory::progressive_retrieval),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/context",
            post(handlers::memory::retrieve_session_context),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents/:agent_id/memory-sessions/:session_id/feedback",
            post(handlers::memory::record_session_feedback),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/audit",
            get(handlers::system::list_audit_events),
        )
        .route(
            "/api/v1/sim/devices/:device_id/telemetry",
            get(handlers::robotics::list_simulated_telemetry)
                .post(handlers::robotics::ingest_simulated_telemetry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/members",
            post(handlers::projects::add_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/members/:target_user_id",
            delete(handlers::projects::revoke_workspace_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects",
            get(handlers::projects::list_projects).post(handlers::projects::create_project),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members",
            post(handlers::projects::set_project_member),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/projects/:project_id/members/:target_user_id",
            delete(handlers::projects::revoke_project_member),
        )
        .route(
            "/api/v1/workspaces",
            get(handlers::system::list_workspaces).post(handlers::system::create_workspace),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/agents",
            get(handlers::system::list_agents).post(handlers::system::register_agent),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/artifacts",
            get(handlers::system::list_artifacts).post(handlers::system::create_artifact),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/tasks",
            get(handlers::tasks::list_tasks).post(handlers::tasks::create_task),
        )
        .route(
            "/api/v1/tasks/:task_id/submit",
            post(handlers::tasks::submit_task),
        )
        .route(
            "/api/v1/tasks/:task_id/execute",
            post(handlers::tasks::execute_task),
        )
        .route(
            "/api/v1/tasks/:task_id/transition",
            post(handlers::tasks::transition_task),
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
