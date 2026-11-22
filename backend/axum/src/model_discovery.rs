use crate::{
    audit::AuditEvent,
    model_registry::{ModelChangeFlag, ModelProvider, ModelRecord, RegistryStatus},
    storage::{EntityKind, Storage, StorageMutation, StoredEntity},
};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc, time::Duration};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogModel {
    pub model_name: String,
    #[serde(default)]
    pub abilities: BTreeSet<String>,
    #[serde(default)]
    pub categories: BTreeSet<String>,
    pub context_length: u64,
    #[serde(default)]
    pub typical_latency_ms: u64,
    #[serde(default)]
    pub regions: BTreeSet<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub retired: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CatalogPrice {
    pub model_name: String,
    pub input_cost_per_million: u64,
    pub output_cost_per_million: u64,
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("provider has no configured model discovery source")]
    SourceNotConfigured,
    #[error("provider API key environment variable is not configured")]
    MissingCredential,
    #[error("configured catalog source request failed")]
    RequestFailed,
    #[error("configured catalog source returned invalid data")]
    InvalidCatalog,
    #[error("registry storage operation failed")]
    StorageFailed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub models_seen: usize,
    pub candidates_added: usize,
    pub changes_flagged: usize,
}

#[async_trait]
pub trait ModelCatalogSource: Send + Sync {
    async fn discover(&self, provider: &ModelProvider) -> Result<Vec<CatalogModel>, DiscoveryError>;
    async fn prices(&self, provider: &ModelProvider) -> Result<Vec<CatalogPrice>, DiscoveryError>;
}

/// Fetches only explicitly configured JSON endpoints. It does not search or
/// crawl websites. The optional credential is read from server environment.
#[derive(Clone, Default)]
pub struct ConfiguredHttpCatalog {
    client: reqwest::Client,
}

impl ConfiguredHttpCatalog {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        provider: &ModelProvider,
        url: &str,
    ) -> Result<T, DiscoveryError> {
        let mut request = self.client.get(url);
        if let Some(env_name) = &provider.api_key_env {
            let key = std::env::var(env_name).map_err(|_| DiscoveryError::MissingCredential)?;
            request = request.bearer_auth(key);
        }
        let response = request.send().await.map_err(|_| DiscoveryError::RequestFailed)?;
        if !response.status().is_success() { return Err(DiscoveryError::RequestFailed); }
        response.json::<T>().await.map_err(|_| DiscoveryError::InvalidCatalog)
    }
}

#[async_trait]
impl ModelCatalogSource for ConfiguredHttpCatalog {
    async fn discover(&self, provider: &ModelProvider) -> Result<Vec<CatalogModel>, DiscoveryError> {
        let url = provider.model_discovery_url.as_deref().ok_or(DiscoveryError::SourceNotConfigured)?;
        self.get_json(provider, url).await
    }

    async fn prices(&self, provider: &ModelProvider) -> Result<Vec<CatalogPrice>, DiscoveryError> {
        match provider.pricing_source_url.as_deref() {
            Some(url) => self.get_json(provider, url).await,
            None => Ok(Vec::new()),
        }
    }
}

fn discovery_audit(event_type: &str, entity_type: &str, id: Uuid, metadata: serde_json::Value) -> AuditEvent {
    AuditEvent::attributed(
        "system:model_discovery", None, event_type, entity_type, id.to_string(), metadata,
    )
}

async fn apply(
    storage: &Arc<dyn Storage>,
    entity: StoredEntity,
    related_entities: Vec<StoredEntity>,
    event: AuditEvent,
) -> Result<(), DiscoveryError> {
    storage.apply(StorageMutation { entity, related_entities, audit: event })
        .await.map_err(|_| DiscoveryError::StorageFailed)
}

pub async fn refresh_provider_catalog(
    storage: &Arc<dyn Storage>,
    source: &dyn ModelCatalogSource,
    provider: &ModelProvider,
) -> Result<DiscoveryReport, DiscoveryError> {
    if !provider.discovery_enabled || provider.status != "active" {
        return Ok(DiscoveryReport::default());
    }
    let discovered = source.discover(provider).await?;
    let prices: BTreeMap<String, CatalogPrice> = source.prices(provider).await?
        .into_iter().map(|price| (price.model_name.clone(), price)).collect();
    let existing: Vec<ModelRecord> = storage.list_entities(EntityKind::Model, None).await
        .map_err(|_| DiscoveryError::StorageFailed)?
        .into_iter().filter_map(|entity| if let StoredEntity::Model(model) = entity {
            (model.provider_id == provider.id).then_some(model)
        } else { None }).collect();

    let mut report = DiscoveryReport { models_seen: discovered.len(), ..DiscoveryReport::default() };
    let seen: BTreeSet<String> = discovered.iter().map(|item| item.model_name.clone()).collect();
    for item in discovered {
        if item.model_name.trim().is_empty() || item.categories.is_empty() || item.abilities.is_empty() || item.context_length == 0 {
            return Err(DiscoveryError::InvalidCatalog);
        }
        let price = prices.get(&item.model_name);
        let current = existing.iter().find(|model| model.model_name == item.model_name);
        if let Some(current) = current {
            let mut updated = current.clone();
            let mut flags = Vec::new();
            if item.retired {
                if !current.flagged_for_review {
                    flags.push(ModelChangeFlag {
                        id: Uuid::new_v4(), model_id: current.id, change_kind: "retired".into(),
                        previous_value: Some("active".into()), observed_value: Some("retired".into()),
                        detected_at: Utc::now(), cleared_at: None, cleared_by: None, resolution_note: None,
                    });
                }
            } else if let Some(price) = price {
                if price.input_cost_per_million != current.input_cost_per_million
                    || price.output_cost_per_million != current.output_cost_per_million {
                    flags.push(ModelChangeFlag {
                        id: Uuid::new_v4(), model_id: current.id, change_kind: "price_changed".into(),
                        previous_value: Some(format!("input={},output={}", current.input_cost_per_million, current.output_cost_per_million)),
                        observed_value: Some(format!("input={},output={}", price.input_cost_per_million, price.output_cost_per_million)),
                        detected_at: Utc::now(), cleared_at: None, cleared_by: None, resolution_note: None,
                    });
                }
            }
            if item.version.is_some() && item.version != current.version {
                flags.push(ModelChangeFlag {
                    id: Uuid::new_v4(), model_id: current.id, change_kind: "version_changed".into(),
                    previous_value: current.version.clone(), observed_value: item.version.clone(),
                    detected_at: Utc::now(), cleared_at: None, cleared_by: None, resolution_note: None,
                });
            }
            if !flags.is_empty() {
                updated.flagged_for_review = true;
                updated.healthy = false;
                updated.updated_at = Utc::now();
                let related = flags.iter().cloned().map(StoredEntity::ModelChangeFlag).collect();
                for flag in &flags {
                    apply(storage, StoredEntity::Model(updated.clone()), related.clone(),
                        discovery_audit("model.change_flagged", "model", current.id,
                            serde_json::json!({"change_kind": flag.change_kind, "flag_id": flag.id}))).await?;
                    report.changes_flagged += 1;
                }
            } else {
                updated.last_seen_at = Utc::now();
                updated.healthy = updated.status == RegistryStatus::Approved && !updated.flagged_for_review;
                updated.updated_at = Utc::now();
                apply(storage, StoredEntity::Model(updated), Vec::new(),
                    discovery_audit("model.catalog_refreshed", "model", current.id, serde_json::json!({"provider_id": provider.id}))).await?;
            }
        } else {
            let price = price.cloned().unwrap_or(CatalogPrice {
                model_name: item.model_name.clone(), input_cost_per_million: 0, output_cost_per_million: 0,
            });
            let now = Utc::now();
            let model = ModelRecord {
                id: Uuid::new_v4(), provider_id: provider.id, model_name: item.model_name,
                status: RegistryStatus::Candidate, abilities: item.abilities, categories: item.categories,
                context_length: item.context_length, input_cost_per_million: price.input_cost_per_million,
                output_cost_per_million: price.output_cost_per_million, typical_latency_ms: item.typical_latency_ms,
                regions: item.regions, version: item.version, last_seen_at: now, notes: "Discovered; evaluation and explicit admin approval required.".into(),
                curated_rank: BTreeMap::new(), healthy: false, flagged_for_review: false,
                created_at: now, updated_at: now,
            };
            apply(storage, StoredEntity::Model(model.clone()), Vec::new(),
                discovery_audit("model.candidate_discovered", "model", model.id,
                    serde_json::json!({"provider_id": provider.id, "model_name": model.model_name}))).await?;
            report.candidates_added += 1;
        }
    }

    for current in existing.iter().filter(|model| !seen.contains(&model.model_name) && !model.flagged_for_review) {
        let mut retired = current.clone();
        retired.flagged_for_review = true;
        retired.healthy = false;
        retired.updated_at = Utc::now();
        let flag = ModelChangeFlag {
            id: Uuid::new_v4(), model_id: current.id, change_kind: "retired".into(),
            previous_value: Some("listed".into()), observed_value: Some("missing_from_catalog".into()),
            detected_at: Utc::now(), cleared_at: None, cleared_by: None, resolution_note: None,
        };
        apply(storage, StoredEntity::Model(retired), vec![StoredEntity::ModelChangeFlag(flag.clone())],
            discovery_audit("model.retirement_flagged", "model", current.id,
                serde_json::json!({"flag_id": flag.id}))).await?;
        report.changes_flagged += 1;
    }
    Ok(report)
}

pub async fn run_discovery_scheduler(storage: Arc<dyn Storage>) {
    let source = ConfiguredHttpCatalog::new();
    let mut schedule = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    loop {
        schedule.tick().await;
        let providers: Vec<ModelProvider> = match storage.list_entities(EntityKind::Provider, None).await {
            Ok(entities) => entities.into_iter().filter_map(|entity| if let StoredEntity::Provider(provider) = entity {
                (provider.discovery_enabled && provider.status == "active").then_some(provider)
            } else { None }).collect(),
            Err(_) => continue,
        };
        for provider in providers {
            if refresh_provider_catalog(&storage, &source, &provider).await.is_err() {
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
        model_registry::{ProviderDataTerms, ModelStat},
        storage::{SqliteStorage, StorageMutation},
    };

    struct FakeCatalog {
        models: Vec<CatalogModel>,
        prices: Vec<CatalogPrice>,
    }

    #[async_trait]
    impl ModelCatalogSource for FakeCatalog {
        async fn discover(&self, _provider: &ModelProvider) -> Result<Vec<CatalogModel>, DiscoveryError> {
            Ok(self.models.clone())
        }
        async fn prices(&self, _provider: &ModelProvider) -> Result<Vec<CatalogPrice>, DiscoveryError> {
            Ok(self.prices.clone())
        }
    }

    fn provider() -> ModelProvider {
        ModelProvider {
            id: Uuid::new_v4(), name: "Test Provider".into(), base_url: "https://provider.test".into(),
            model_discovery_url: Some("https://catalog.test/models.json".into()),
            pricing_source_url: Some("https://catalog.test/prices.json".into()),
            api_key_env: None, discovery_enabled: true, rate_limit_per_minute: 10,
            status: "active".into(),
            data_terms: ProviderDataTerms { processing_regions: vec!["eu".into()],
                trains_on_api_inputs: Some(false), retention_days: Some(0), retention_description: "No retention".into() },
            terms_reviewed_at: Some(Utc::now()), reviewed_by: Some(Uuid::new_v4()),
            created_at: Utc::now(), updated_at: Utc::now(),
        }
    }

    fn catalog_model(name: &str) -> CatalogModel {
        CatalogModel {
            model_name: name.into(), abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(), context_length: 16_000,
            typical_latency_ms: 500, regions: ["eu".to_string()].into_iter().collect(),
            version: Some("v1".into()), retired: false,
        }
    }

    async fn save(storage: &Arc<dyn Storage>, entity: StoredEntity) {
        storage.apply(StorageMutation {
            entity, related_entities: Vec::new(),
            audit: AuditEvent::unattributed(None, "test.seeded", "test", "seed", serde_json::Value::Null),
        }).await.unwrap();
    }

    #[tokio::test]
    async fn newly_discovered_models_are_candidates_and_never_approved() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let source = FakeCatalog { models: vec![catalog_model("new-model")],
            prices: vec![CatalogPrice { model_name: "new-model".into(), input_cost_per_million: 10, output_cost_per_million: 20 }] };
        let report = refresh_provider_catalog(&storage, &source, &provider).await.unwrap();
        assert_eq!(report.candidates_added, 1);
        let models = storage.list_entities(EntityKind::Model, None).await.unwrap();
        assert!(matches!(&models[0], StoredEntity::Model(model) if model.status == RegistryStatus::Candidate && !model.healthy));
    }

    #[tokio::test]
    async fn approved_model_price_changes_are_flagged_without_silent_price_updates() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let now = Utc::now();
        let model = ModelRecord {
            id: Uuid::new_v4(), provider_id: provider.id, model_name: "existing".into(),
            status: RegistryStatus::Approved, abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(), context_length: 16_000,
            input_cost_per_million: 10, output_cost_per_million: 20, typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(), version: Some("v1".into()),
            last_seen_at: now, notes: String::new(), curated_rank: BTreeMap::new(), healthy: true,
            flagged_for_review: false, created_at: now, updated_at: now,
        };
        save(&storage, StoredEntity::Model(model.clone())).await;
        let source = FakeCatalog { models: vec![catalog_model("existing")],
            prices: vec![CatalogPrice { model_name: "existing".into(), input_cost_per_million: 99, output_cost_per_million: 88 }] };
        let report = refresh_provider_catalog(&storage, &source, &provider).await.unwrap();
        assert_eq!(report.changes_flagged, 1);
        let models = storage.list_entities(EntityKind::Model, None).await.unwrap();
        assert!(matches!(&models[0], StoredEntity::Model(updated) if updated.flagged_for_review && !updated.healthy && updated.input_cost_per_million == 10 && updated.output_cost_per_million == 20));
        let flags = storage.list_entities(EntityKind::ModelChangeFlag, None).await.unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "price_changed")));
    }

    #[tokio::test]
    async fn model_missing_from_configured_catalog_is_flagged_as_retired() {
        let storage: Arc<dyn Storage> = SqliteStorage::connect_in_memory().await.unwrap().shared();
        let provider = provider();
        save(&storage, StoredEntity::Provider(provider.clone())).await;
        let now = Utc::now();
        let model = ModelRecord {
            id: Uuid::new_v4(), provider_id: provider.id, model_name: "retired-model".into(),
            status: RegistryStatus::Approved, abilities: ["text".to_string()].into_iter().collect(),
            categories: ["general".to_string()].into_iter().collect(), context_length: 16_000,
            input_cost_per_million: 10, output_cost_per_million: 20, typical_latency_ms: 500,
            regions: ["eu".to_string()].into_iter().collect(), version: Some("v1".into()),
            last_seen_at: now, notes: String::new(), curated_rank: BTreeMap::new(), healthy: true,
            flagged_for_review: false, created_at: now, updated_at: now,
        };
        save(&storage, StoredEntity::Model(model)).await;
        let source = FakeCatalog { models: Vec::new(), prices: Vec::new() };
        let report = refresh_provider_catalog(&storage, &source, &provider).await.unwrap();
        assert_eq!(report.changes_flagged, 1);
        let flags = storage.list_entities(EntityKind::ModelChangeFlag, None).await.unwrap();
        assert!(flags.iter().any(|entity| matches!(entity, StoredEntity::ModelChangeFlag(flag) if flag.change_kind == "retired")));
    }
}
