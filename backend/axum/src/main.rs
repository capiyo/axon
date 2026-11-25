use axon_api::{
    app,
    firebase_auth::FirebaseVerifier,
    memory_documents::{MemoryDocumentRepository, MongoMemoryDocumentRepository},
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

    let database_url =
        env::var("AXON_DATABASE_URL").unwrap_or_else(|_| "sqlite://axon.db".to_owned());
    let storage = SqliteStorage::connect(&database_url).await?;
    let memory_epochs = storage.memory_epoch_coordinator();
    tokio::spawn(memory_epochs.run_hourly());
    let memory_outbox = storage.memory_outbox();

    let mongo_required = env::var("AXON_MEMORY_DOCUMENTS_REQUIRED")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or_else(|_| {
            env::var("AXON_ENV")
                .map(|value| value.eq_ignore_ascii_case("production"))
                .unwrap_or(false)
        });
    let memory_documents: Option<Arc<dyn MemoryDocumentRepository>> =
        match env::var("AXON_MONGODB_URI") {
            Ok(uri) => {
                let database = env::var("AXON_MONGODB_DATABASE")
                    .unwrap_or_else(|_| "axon".to_owned());
                let collection = env::var("AXON_MONGODB_COLLECTION")
                    .unwrap_or_else(|_| "memory_documents".to_owned());
                let repository =
                    MongoMemoryDocumentRepository::connect(&uri, &database, &collection).await?;
                tracing::info!("MongoDB memory document repository connected");
                Some(Arc::new(repository))
            }
            Err(_) if mongo_required => {
                return Err(
                    "AXON_MONGODB_URI is required when AXON_ENV=production or AXON_MEMORY_DOCUMENTS_REQUIRED=true"
                        .into(),
                );
            }
            Err(_) => {
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
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    if let Some(repository) = memory_documents.clone() {
        tokio::spawn(memory_outbox.clone().run(
            repository,
            format!("axon-memory-outbox-{}", uuid::Uuid::new_v4()),
            shutdown_rx,
        ));
    }

    let state = AppState::with_storage(storage.shared())
        .await?
        .with_memory_document_repository(memory_documents)
        .with_memory_outbox(Some(memory_outbox))
        .with_qoin_vector_client(qoin_vectors)
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
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = shutdown_tx.send(true);
        })
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
