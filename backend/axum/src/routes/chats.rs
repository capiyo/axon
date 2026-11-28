//! Chats HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/workspaces/:workspace_id/chats",
            routing::get(handlers::chats::list_chats).routing::post(handlers::chats::create_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id",
            routing::get(handlers::chats::get_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/read",
            routing::post(handlers::chats::mark_chat_read),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/like",
            routing::post(handlers::chats::toggle_chat_like),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages",
            routing::post(handlers::chats::append_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/archive",
            routing::patch(handlers::chats::archive_chat),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory",
            routing::post(handlers::chats::create_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory/:entry_id/:action",
            routing::post(handlers::chats::mutate_memory_entry),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/memory-index",
            routing::post(handlers::memory::index_chat_message),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/memory-index/:entry_id/qoin",
            routing::post(handlers::memory::index_memory_with_qoin),
        )
        .route(
            "/api/v1/workspaces/:workspace_id/chats/:chat_id/messages/:message_id/qoin-index",
            routing::post(handlers::memory::index_chat_message_with_qoin),
        )
}
