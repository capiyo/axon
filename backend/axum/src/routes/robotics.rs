//! Robotics HTTP endpoints.
use crate::{handlers, AppState};
use axum::{routing, Router};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/sim/devices/:device_id/telemetry",
            routing::get(handlers::robotics::list_simulated_telemetry)
                .routing::post(handlers::robotics::ingest_simulated_telemetry),
        )
}
