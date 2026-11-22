use axon_api::{
    app, firebase_auth::FirebaseVerifier, model_discovery::run_discovery_scheduler,
    storage::SqliteStorage, AppState,
};
use std::{env, net::SocketAddr};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "axon_api=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let project_id = env::var("FIREBASE_PROJECT_ID").map_err(|_| {
        "FIREBASE_PROJECT_ID must be configured; refusing to start without Firebase authentication"
    })?;
    let verifier = FirebaseVerifier::new(project_id)?;
    let host = env::var("AXON_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("AXON_PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()?;
    let address: SocketAddr = format!("{host}:{port}").parse()?;

    let database_url =
        env::var("AXON_DATABASE_URL").unwrap_or_else(|_| "sqlite://axon.db".to_owned());
    let storage = SqliteStorage::connect(&database_url).await?;
    let storage_handle = storage.shared();
    let simulator_enabled = env::var("AXON_ENABLE_SIMULATOR")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let admin_uids = env::var("AXON_ADMIN_UIDS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let state = AppState::with_storage(storage_handle.clone())
        .await?
        .with_platform_admin_uids(admin_uids)
        .with_firebase_verifier(verifier, simulator_enabled);
    tokio::spawn(run_discovery_scheduler(storage_handle));

    let listener = TcpListener::bind(address).await?;
    tracing::info!(
        %address,
        storage = "sqlite",
        simulator_enabled,
        "Axon API listening with Firebase authentication and durable SQLite storage"
    );
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
