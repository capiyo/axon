use axon_api::{
    app,
    config::BackendConfig,
    explore::ExploreStore,
    firebase_auth::FirebaseVerifier,
    memory_documents::{MemoryDocumentRepository, MongoMemoryDocumentRepository},
    memory_vector_provider::LexicalMemoryVectorProvider,
    qoin_vectors::QoinVectorClient,
    storage::SqliteStorage,
    AppState,
};
use std::{env, net::SocketAddr, sync::Arc};
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

    let config = BackendConfig::from_env()?;
    let storage = SqliteStorage::connect(&config.database_url).await?;
    let explore_store = Arc::new(ExploreStore::new(storage.sqlite_pool()));
    let memory_epochs = storage
        .memory_epoch_coordinator()
        .with_qoin_scoring_version(config.qoin_memory_scoring.effective_version());
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let epoch_worker = tokio::spawn(memory_epochs.run_hourly_until(shutdown_rx.clone()));
    let memory_outbox = storage.memory_outbox();

    let memory_documents: Option<Arc<dyn MemoryDocumentRepository>> = match config.mongo {
        Some(settings) => {
            let repository = MongoMemoryDocumentRepository::connect_with_options(
                    &settings.uri,
                    &settings.database,
                    &settings.collection,
                    settings.connect_timeout,
                    settings.server_selection_timeout,
                    settings.max_pool_size,
                )
                .await
                .map_err(|_| {
                    "MongoDB memory document connection or health check failed; verify the configured URI, network, and TLS settings"
                })?;
            tracing::info!(
                database = %settings.database,
                collection = %settings.collection,
                max_pool_size = settings.max_pool_size,
                "MongoDB memory document repository connected"
            );
            Some(Arc::new(repository))
        }
        None => {
            tracing::warn!(
                "MongoDB memory documents are disabled; SQLite-only mode remains active"
            );
            None
        }
    };

    let simulator_enabled = env::var("AXON_ENABLE_SIMULATOR")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let qoin_vectors = env::var("QOIN_API_URL")
        .ok()
        .map(|url| QoinVectorClient::new(&url))
        .transpose()?;
    let outbox_worker = memory_documents.clone().map(|repository| {
        tokio::spawn(memory_outbox.clone().run(
            repository,
            format!("axon-memory-outbox-{}", uuid::Uuid::new_v4()),
            shutdown_rx,
        ))
    });

    let state = AppState::with_storage(storage.shared())
        .await?
        .with_explore_store(explore_store)
        .with_memory_document_repository(memory_documents)
        .with_memory_outbox(Some(memory_outbox))
        .with_qoin_vector_client(qoin_vectors)
        .with_qoin_memory_scoring_config(config.qoin_memory_scoring)
        .with_memory_vector_provider(Some(Arc::new(LexicalMemoryVectorProvider)))
        .with_firebase_verifier(verifier, simulator_enabled);

    let listener = TcpListener::bind(address).await?;
    tracing::info!(
        %address,
        storage = "sqlite",
        mongo_memory_documents = state.memory_documents.is_some(),
        simulator_enabled,
        qoin_vectors = env::var("QOIN_API_URL").is_ok(),
        "Axon API listening with Firebase authentication and durable SQLite storage"
    );
    let serve_result = axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            shutdown_signal().await;
        })
        .await;

    let _ = shutdown_tx.send(true);
    // Both workers leave unclaimed work durable in SQLite. Bound shutdown
    // waiting so a stalled provider cannot prevent process termination.
    if let Some(worker) = outbox_worker {
        match tokio::time::timeout(std::time::Duration::from_secs(5), worker).await {
            Ok(Ok(())) => tracing::info!("memory outbox worker stopped cleanly"),
            Ok(Err(error)) => tracing::error!(error = %error, "memory outbox worker task failed"),
            Err(_) => tracing::warn!("memory outbox worker did not stop within five seconds; pending events remain recoverable"),
        }
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), epoch_worker).await {
        Ok(Ok(())) => tracing::info!("memory epoch worker stopped cleanly"),
        Ok(Err(error)) => tracing::error!(error = %error, "memory epoch worker task failed"),
        Err(_) => tracing::warn!("memory epoch worker did not stop within five seconds; next process can republish due epochs"),
    }
    serve_result?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
