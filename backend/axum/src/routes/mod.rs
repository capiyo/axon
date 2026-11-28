//! HTTP route composition and cross-cutting middleware.
mod admin;
mod agents;
mod capabilities;
mod chats;
mod explore;
mod memory;
mod projects;
mod robotics;
mod supervisor;
mod system;
mod tasks;

use crate::{auth_middleware, AppState};
use axum::{http::{header, HeaderValue, Method}, middleware, Router};
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
        .merge(system::router())
        .merge(admin::router())
        .merge(capabilities::router())
        .merge(supervisor::router())
        .merge(explore::router())
        .merge(chats::router())
        .merge(memory::router())
        .merge(agents::router())
        .merge(projects::router())
        .merge(tasks::router())
        .merge(robotics::router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::authenticate,
        ))
        .with_state(state)
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
}
