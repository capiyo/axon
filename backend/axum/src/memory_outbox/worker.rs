impl MemoryOutbox {
    pub async fn run(
        self,
        repository: Arc<dyn MemoryDocumentRepository>,
        worker_id: String,
        mut shutdown: watch::Receiver<bool>,
    ) {
        loop {
            if *shutdown.borrow() {
                break;
            }
            match self.claim_next(&worker_id).await {
                Ok(Some(event)) => {
                    let result: Result<(), MemoryOutboxError> = async {
                        repository.put_version(event.document.clone()).await?;
                        let verified = repository
                            .verify_version(
                                event.document.memory_id,
                                event.document.version,
                                &event.document.content_hash,
                            )
                            .await?;
                        if !verified {
                            return Err(MemoryOutboxError::VersionConflict);
                        }
                        self.complete(&event, &worker_id).await
                    }
                    .await;
                    if let Err(error) = result {
                        tracing::warn!(
                            event_id = %event.event_id,
                            error = %error,
                            "memory document outbox event failed"
                        );
                        if let Err(fail_error) = self
                            .fail(
                                event.event_id,
                                &worker_id,
                                "MongoDB persistence or verification failed",
                            )
                            .await
                        {
                            tracing::error!(
                                event_id = %event.event_id,
                                error = %fail_error,
                                "could not update memory outbox retry state"
                            );
                        }
                    }
                }
                Ok(None) => {
                    tokio::select! {
                        _ = sleep(Duration::from_millis(500)) => {},
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(error = %error, "memory outbox polling failed");
                    tokio::select! {
                        _ = sleep(Duration::from_secs(2)) => {},
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
}
