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
    async fn discover(&self, provider: &ModelProvider)
        -> Result<Vec<CatalogModel>, DiscoveryError>;
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
        Self {
            client: reqwest::Client::new(),
        }
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
        let response = request
            .send()
            .await
            .map_err(|_| DiscoveryError::RequestFailed)?;
        if !response.status().is_success() {
            return Err(DiscoveryError::RequestFailed);
        }
        response
            .json::<T>()
            .await
            .map_err(|_| DiscoveryError::InvalidCatalog)
    }
}

#[async_trait]
impl ModelCatalogSource for ConfiguredHttpCatalog {
    async fn discover(
        &self,
        provider: &ModelProvider,
    ) -> Result<Vec<CatalogModel>, DiscoveryError> {
        let url = provider
            .model_discovery_url
            .as_deref()
            .ok_or(DiscoveryError::SourceNotConfigured)?;
        self.get_json(provider, url).await
    }

    async fn prices(&self, provider: &ModelProvider) -> Result<Vec<CatalogPrice>, DiscoveryError> {
        match provider.pricing_source_url.as_deref() {
            Some(url) => self.get_json(provider, url).await,
            None => Ok(Vec::new()),
        }
    }
}

