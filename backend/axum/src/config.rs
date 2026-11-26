//! Validated backend configuration. Secret-bearing values intentionally do not
//! implement a revealing Debug representation.

use std::{env, fmt, time::Duration};
use thiserror::Error;

use crate::memory_intelligence::QoinScoringConfig;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0} must be a valid boolean (true or false)")]
    InvalidBoolean(&'static str),
    #[error("{0} must be an integer in the supported range")]
    InvalidInteger(&'static str),
    #[error("{0} must not be empty")]
    EmptyValue(&'static str),
    #[error("AXON_MONGODB_URI is required when MongoDB memory documents are required")]
    MissingMongoUri,
    #[error("AXON_QOIN_MEMORY_SCORING_CONFIG must contain a valid versioned Qoin scoring configuration")]
    InvalidQoinScoringConfig,
}

#[derive(Clone)]
pub struct MongoSettings {
    pub uri: String,
    pub database: String,
    pub collection: String,
    pub connect_timeout: Duration,
    pub server_selection_timeout: Duration,
    pub max_pool_size: u32,
}

impl fmt::Debug for MongoSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MongoSettings")
            .field("uri", &"[REDACTED]")
            .field("database", &self.database)
            .field("collection", &self.collection)
            .field("connect_timeout", &self.connect_timeout)
            .field("server_selection_timeout", &self.server_selection_timeout)
            .field("max_pool_size", &self.max_pool_size)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct BackendConfig {
    pub database_url: String,
    pub mongo: Option<MongoSettings>,
    pub qoin_memory_scoring: QoinScoringConfig,
}

impl BackendConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("AXON_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://axon.db".to_owned());
        if database_url.trim().is_empty() {
            return Err(ConfigError::EmptyValue("AXON_DATABASE_URL"));
        }

        let environment = env::var("AXON_ENV").unwrap_or_default();
        let required = match env::var("AXON_MEMORY_DOCUMENTS_REQUIRED") {
            Ok(value) => parse_bool("AXON_MEMORY_DOCUMENTS_REQUIRED", &value)?,
            Err(_) => environment.eq_ignore_ascii_case("production"),
        };
        let mongo_uri = env::var("AXON_MONGODB_URI").ok();
        let mongo = match mongo_uri {
            Some(uri) => {
                if uri.trim().is_empty() {
                    return Err(ConfigError::EmptyValue("AXON_MONGODB_URI"));
                }
                let database = env::var("AXON_MONGODB_DATABASE")
                    .unwrap_or_else(|_| "axon".to_owned());
                let collection = env::var("AXON_MONGODB_COLLECTION")
                    .unwrap_or_else(|_| "memory_documents".to_owned());
                if database.trim().is_empty() {
                    return Err(ConfigError::EmptyValue("AXON_MONGODB_DATABASE"));
                }
                if collection.trim().is_empty() {
                    return Err(ConfigError::EmptyValue("AXON_MONGODB_COLLECTION"));
                }
                Some(MongoSettings {
                    uri,
                    database,
                    collection,
                    connect_timeout: duration_from_env(
                        "AXON_MONGODB_CONNECT_TIMEOUT_MS",
                        10_000,
                    )?,
                    server_selection_timeout: duration_from_env(
                        "AXON_MONGODB_SERVER_SELECTION_TIMEOUT_MS",
                        5_000,
                    )?,
                    max_pool_size: integer_from_env("AXON_MONGODB_MAX_POOL_SIZE", 20, 1, 500)?,
                })
            }
            None if required => return Err(ConfigError::MissingMongoUri),
            None => None,
        };

        let qoin_memory_scoring = match env::var("AXON_QOIN_MEMORY_SCORING_CONFIG") {
            Ok(serialized) => {
                let policy: QoinScoringConfig = serde_json::from_str(&serialized)
                    .map_err(|_| ConfigError::InvalidQoinScoringConfig)?;
                policy.validate().map_err(|_| ConfigError::InvalidQoinScoringConfig)?;
                policy
            }
            Err(_) => QoinScoringConfig::default(),
        };

        Ok(Self {
            database_url,
            mongo,
            qoin_memory_scoring,
        })
    }
}

fn parse_bool(name: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(ConfigError::InvalidBoolean(name)),
    }
}

fn integer_from_env(
    name: &'static str,
    default: u32,
    minimum: u32,
    maximum: u32,
) -> Result<u32, ConfigError> {
    match env::var(name) {
        Ok(value) => {
            let parsed = value
                .parse::<u32>()
                .map_err(|_| ConfigError::InvalidInteger(name))?;
            if !(minimum..=maximum).contains(&parsed) {
                return Err(ConfigError::InvalidInteger(name));
            }
            Ok(parsed)
        }
        Err(_) => Ok(default),
    }
}

fn duration_from_env(name: &'static str, default_ms: u64) -> Result<Duration, ConfigError> {
    let millis = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidInteger(name))?,
        Err(_) => default_ms,
    };
    if !(100..=120_000).contains(&millis) {
        return Err(ConfigError::InvalidInteger(name));
    }
    Ok(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_parser_accepts_explicit_values_and_rejects_ambiguity() {
        assert_eq!(parse_bool("FLAG", "true"), Ok(true));
        assert_eq!(parse_bool("FLAG", "NO"), Ok(false));
        assert_eq!(parse_bool("FLAG", "sometimes"), Err(ConfigError::InvalidBoolean("FLAG")));
    }

    #[test]
    fn pool_size_is_bounded() {
        assert_eq!(integer_from_env_value("POOL", "24", 20, 1, 500), Ok(24));
        assert!(integer_from_env_value("POOL", "0", 20, 1, 500).is_err());
        assert!(integer_from_env_value("POOL", "501", 20, 1, 500).is_err());
    }

    fn integer_from_env_value(
        name: &'static str,
        value: &str,
        _default: u32,
        minimum: u32,
        maximum: u32,
    ) -> Result<u32, ConfigError> {
        let parsed = value.parse::<u32>().map_err(|_| ConfigError::InvalidInteger(name))?;
        if !(minimum..=maximum).contains(&parsed) {
            return Err(ConfigError::InvalidInteger(name));
        }
        Ok(parsed)
    }

    #[test]
    fn mongo_debug_output_never_contains_uri_credentials() {
        let settings = MongoSettings {
            uri: "mongodb://secret-user:secret-password@db.example/axon".into(),
            database: "axon".into(),
            collection: "memory_documents".into(),
            connect_timeout: Duration::from_secs(10),
            server_selection_timeout: Duration::from_secs(5),
            max_pool_size: 20,
        };
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("secret-user"));
        assert!(!rendered.contains("secret-password"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
