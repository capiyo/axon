//! Admin HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/admin/providers",
            routing::get(handlers::model_registry::list_providers).post(handlers::model_registry::create_provider),
        )
        .route(
            "/api/v1/admin/providers/:provider_id/terms-review",
            routing::post(handlers::model_registry::review_provider_terms),
        )
        .route(
            "/api/v1/admin/models",
            routing::get(handlers::model_registry::list_models).post(handlers::model_registry::create_model),
        )
        .route(
            "/api/v1/admin/models/:model_id/review",
            routing::post(handlers::model_registry::review_candidate),
        )
        .route("/api/v1/admin/evaluations", routing::get(handlers::model_registry::list_evaluations))
        .route("/api/v1/admin/stats", routing::get(handlers::model_registry::list_stats))
        .route("/api/v1/admin/flags", routing::get(handlers::model_registry::list_flags))
        .route("/api/v1/admin/flags/:flag_id/clear", routing::post(handlers::model_registry::clear_flag))
        .route("/api/v1/admin/assignments", routing::get(handlers::model_registry::list_assignments))
}
