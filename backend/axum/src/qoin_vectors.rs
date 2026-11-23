use crate::memory_architecture::QoinVector;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const QOIN_VECTOR_DIMENSION: usize = 6;

#[derive(Debug, Error)]
pub enum QoinVectorError {
    #[error("invalid QOIN_API_URL: {0}")]
    InvalidBaseUrl(String),
    #[error("Qoin vector service request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Qoin vector service returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("Qoin has no current vector for this symbol")]
    MissingVector,
    #[error("Qoin vector must contain exactly {QOIN_VECTOR_DIMENSION} finite attended features")]
    InvalidVector,
    #[error("Qoin response is missing its strategy version")]
    MissingModelVersion,
}

#[derive(Debug, Clone, Serialize)]
pub struct QoinVectorSnapshot {
    pub symbol: String,
    pub vector: QoinVector,
    pub features: BTreeMap<String, f64>,
    pub action: String,
    pub reason: String,
    pub decision_vector: Option<[f64; 2]>,
}

#[derive(Debug, Clone)]
pub struct QoinVectorClient {
    http: reqwest::Client,
    endpoint: Url,
}

#[derive(Debug, Deserialize)]
struct QoinDecision {
    symbol: String,
    #[serde(default)]
    strategy_version: Option<String>,
    #[serde(default)]
    model_version: Option<String>,
    #[serde(default)]
    attended_features: Option<Vec<f64>>,
    #[serde(default)]
    features: BTreeMap<String, f64>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    decision_vector: Option<[f64; 2]>,
}

impl QoinVectorClient {
    pub fn new(base_url: &str) -> Result<Self, QoinVectorError> {
        let mut endpoint = Url::parse(base_url)
            .map_err(|error| QoinVectorError::InvalidBaseUrl(error.to_string()))?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            return Err(QoinVectorError::InvalidBaseUrl(
                "only absolute HTTP(S) URLs are supported".into(),
            ));
        }
        endpoint.set_path(&format!(
            "{}/internal/vector",
            endpoint.path().trim_end_matches('/')
        ));
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()?,
            endpoint,
        })
    }

    pub async fn latest(&self, symbol: &str) -> Result<QoinVectorSnapshot, QoinVectorError> {
        let symbol = symbol.trim().to_ascii_uppercase();
        if symbol.is_empty() || symbol.len() > 32 {
            return Err(QoinVectorError::MissingVector);
        }
        let response = self
            .http
            .get(self.endpoint.clone())
            .query(&[("symbol", symbol.as_str())])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(QoinVectorError::HttpStatus(response.status()));
        }
        let decision: QoinDecision = response.json().await?;
        if decision.symbol.trim().to_ascii_uppercase() != symbol {
            return Err(QoinVectorError::InvalidVector);
        }
        let values = decision
            .attended_features
            .ok_or(QoinVectorError::MissingVector)?;
        if values.len() != QOIN_VECTOR_DIMENSION || values.iter().any(|value| !value.is_finite()) {
            return Err(QoinVectorError::InvalidVector);
        }
        let values = values.into_iter().map(|value| value as f32).collect::<Vec<_>>();
        if values.iter().any(|value| !value.is_finite()) {
            return Err(QoinVectorError::InvalidVector);
        }
        let model_version = decision
            .strategy_version
            .or(decision.model_version)
            .filter(|version| !version.trim().is_empty())
            .ok_or(QoinVectorError::MissingModelVersion)?;
        let vector = QoinVector {
            values,
            model_version,
        };
        vector
            .validate()
            .map_err(|_| QoinVectorError::InvalidVector)?;
        Ok(QoinVectorSnapshot {
            symbol: decision.symbol,
            vector,
            features: decision.features,
            action: decision.action.unwrap_or_else(|| "wait".into()),
            reason: decision.reason.unwrap_or_default(),
            decision_vector: decision.decision_vector,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qoin_endpoint_is_built_from_service_base_url() {
        let client = QoinVectorClient::new("http://127.0.0.1:8081/").unwrap();
        assert_eq!(
            client.endpoint.as_str(),
            "http://127.0.0.1:8081/internal/vector"
        );
    }

    #[test]
    fn qoin_endpoint_preserves_a_service_prefix() {
        let client = QoinVectorClient::new("http://qoin:8081/research/").unwrap();
        assert_eq!(
            client.endpoint.as_str(),
            "http://qoin:8081/research/internal/vector"
        );
    }
}
