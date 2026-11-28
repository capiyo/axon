pub async fn run_discovery_scheduler(storage: Arc<dyn Storage>) {
    let source = ConfiguredHttpCatalog::new();
    let mut schedule = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    loop {
        schedule.tick().await;
        let providers: Vec<ModelProvider> =
            match storage.list_entities(EntityKind::Provider, None).await {
                Ok(entities) => entities
                    .into_iter()
                    .filter_map(|entity| {
                        if let StoredEntity::Provider(provider) = entity {
                            (provider.discovery_enabled && provider.status == "active")
                                .then_some(provider)
                        } else {
                            None
                        }
                    })
                    .collect(),
                Err(_) => continue,
            };
        for provider in providers {
            if refresh_provider_catalog(&storage, &source, &provider)
                .await
                .is_err()
            {
                tracing::warn!(provider_id = %provider.id, "configured model catalog refresh failed");
            }
            let interval_ms = 60_000u64 / u64::from(provider.rate_limit_per_minute.max(1));
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audit::AuditEvent,
        model_registry::ProviderDataTerms,
        storage::{SqliteStorage, StorageMutation},
    };

