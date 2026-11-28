//! Deterministic memory-key provider used by the QKV document retrieval path.
//!
//! The default implementation is lexical feature hashing, not a neural
//! embedding. A neural provider can implement this trait later without
//! changing the QKV/Qoin ranking pipeline or mixing vector model versions.

use crate::memory_architecture::QoinVector;
use async_trait::async_trait;
use thiserror::Error;

pub const LEXICAL_MEMORY_VECTOR_VERSION: &str = "axon-lexical-qkv-v1";
const DIMENSION: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryVectorProviderError {
    #[error("memory query contains no searchable terms")]
    EmptyQuery,
    #[error("configured memory embedding provider is unavailable")]
    ProviderUnavailable,
    #[error("memory embedding provider returned an invalid vector")]
    InvalidVector,
    #[error("memory embedding provider returned an incompatible model version")]
    ModelVersionMismatch,
}

#[async_trait]
pub trait MemoryVectorProvider: Send + Sync {
    fn model_version(&self) -> &'static str;
    async fn key_for_text(&self, text: &str) -> Result<QoinVector, MemoryVectorProviderError>;
}

pub fn validate_memory_key(
    key: &QoinVector,
    expected_model_version: &str,
) -> Result<(), MemoryVectorProviderError> {
    if key.model_version != expected_model_version {
        return Err(MemoryVectorProviderError::ModelVersionMismatch);
    }
    if key.values.is_empty()
        || key.values.len() > 4096
        || key.values.iter().any(|value| !value.is_finite())
    {
        return Err(MemoryVectorProviderError::InvalidVector);
    }
    let norm = key
        .values
        .iter()
        .map(|value| (*value as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || (norm - 1.0).abs() > 1e-4 {
        return Err(MemoryVectorProviderError::InvalidVector);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LexicalMemoryVectorProvider;

#[async_trait]
impl MemoryVectorProvider for LexicalMemoryVectorProvider {
    fn model_version(&self) -> &'static str {
        LEXICAL_MEMORY_VECTOR_VERSION
    }

    async fn key_for_text(&self, text: &str) -> Result<QoinVector, MemoryVectorProviderError> {
        let mut values = vec![0.0_f32; DIMENSION];
        for token in text
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            // FNV-1a is stable across processes and platforms.
            let mut hash = 0xcbf29ce484222325_u64;
            for byte in token.to_lowercase().bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            let index = (hash as usize) % DIMENSION;
            let sign = if (hash >> 8) & 1 == 0 { 1.0 } else { -1.0 };
            values[index] += sign;
        }

        let norm = values
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt();
        if norm == 0.0 {
            return Err(MemoryVectorProviderError::EmptyQuery);
        }
        for value in &mut values {
            *value = (f64::from(*value) / norm) as f32;
        }

        Ok(QoinVector {
            values,
            model_version: self.model_version().into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lexical_keys_are_stable_and_normalized() {
        let provider = LexicalMemoryVectorProvider;
        let first = provider
            .key_for_text("Axon memory QKV retrieval")
            .await
            .unwrap();
        let second = provider
            .key_for_text("Axon memory QKV retrieval")
            .await
            .unwrap();
        assert_eq!(first.values, second.values);
        assert_eq!(first.values.len(), DIMENSION);
        assert_eq!(first.model_version, LEXICAL_MEMORY_VECTOR_VERSION);
        let norm = first
            .values
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn punctuation_only_text_is_rejected() {
        let provider = LexicalMemoryVectorProvider;
        assert_eq!(
            provider.key_for_text("... !!! ---").await.unwrap_err(),
            MemoryVectorProviderError::EmptyQuery
        );
    }

    #[test]
    fn provider_key_validation_checks_version_dimension_and_normalization() {
        let valid = QoinVector {
            values: vec![0.6, 0.8],
            model_version: "test-v1".into(),
        };
        assert!(validate_memory_key(&valid, "test-v1").is_ok());
        assert_eq!(
            validate_memory_key(&valid, "test-v2"),
            Err(MemoryVectorProviderError::ModelVersionMismatch),
        );
        let invalid_norm = QoinVector {
            values: vec![2.0, 0.0],
            model_version: "test-v1".into(),
        };
        assert_eq!(
            validate_memory_key(&invalid_norm, "test-v1"),
            Err(MemoryVectorProviderError::InvalidVector),
        );
    }

    #[tokio::test]
    async fn unrelated_text_produces_different_keys() {
        let provider = LexicalMemoryVectorProvider;
        let memory = provider
            .key_for_text("workspace memory document retrieval")
            .await
            .unwrap();
        let market = provider
            .key_for_text("volatility liquidity orderbook futures")
            .await
            .unwrap();
        assert_ne!(memory.values, market.values);
    }
}
