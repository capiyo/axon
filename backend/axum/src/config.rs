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
    #[error(
        "AXON_QOIN_MEMORY_SCORING_CONFIG must contain a valid versioned Qoin scoring configuration"
    )]
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
        let database_url =
            env::var("AXON_DATABASE_URL").unwrap_or_else(|_| "sqlite://axon.db".to_owned());
        if database_url.trim().is_empty() {
            return Err(ConfigError::EmptyValue("AXON_DATABASE_URL"));
        }

        let environment = env::var("AXON_ENV").unwrap_or_default();
        // Production cannot silently opt out of the document store. Local and
        // test environments may omit MongoDB by leaving it unconfigured.
        let required = mongo_is_required(
            &environment,
            env::var("AXON_MEMORY_DOCUMENTS_REQUIRED").ok().as_deref(),
        )?;
        let mongo_uri = env::var("AXON_MONGODB_URI").ok();
        let mongo = match mongo_uri {
            Some(uri) => {
                if uri.trim().is_empty() {
                    return Err(ConfigError::EmptyValue("AXON_MONGODB_URI"));
                }
                let database =
                    env::var("AXON_MONGODB_DATABASE").unwrap_or_else(|_| "axon".to_owned());
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
                    connect_timeout: duration_from_env("AXON_MONGODB_CONNECT_TIMEOUT_MS", 10_000)?,
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
            Ok(serialized) => parse_qoin_scoring_config(&serialized)?,
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

fn mongo_is_required(environment: &str, override_value: Option<&str>) -> Result<bool, ConfigError> {
    let explicitly_required = override_value
        .map(|value| parse_bool("AXON_MEMORY_DOCUMENTS_REQUIRED", value))
        .transpose()?
        .unwrap_or(false);
    Ok(environment.eq_ignore_ascii_case("production") || explicitly_required)
}

fn integer_from_env(
    name: &'static str,
    default: u32,
    minimum: u32,
    maximum: u32,
) -> Result<u32, ConfigError> {
    match env::var(name) {
        Ok(value) => parse_bounded_integer(name, &value, minimum, maximum),
        Err(_) => Ok(default),
    }
}

fn parse_bounded_integer(
    name: &'static str,
    value: &str,
    minimum: u32,
    maximum: u32,
) -> Result<u32, ConfigError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| ConfigError::InvalidInteger(name))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(ConfigError::InvalidInteger(name));
    }
    Ok(parsed)
}

fn duration_from_env(name: &'static str, default_ms: u64) -> Result<Duration, ConfigError> {
    match env::var(name) {
        Ok(value) => parse_duration_ms(name, &value),
        Err(_) => Ok(Duration::from_millis(default_ms)),
    }
}

fn parse_duration_ms(name: &'static str, value: &str) -> Result<Duration, ConfigError> {
    let millis = value
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidInteger(name))?;
    if !(100..=120_000).contains(&millis) {
        return Err(ConfigError::InvalidInteger(name));
    }
    Ok(Duration::from_millis(millis))
}

fn parse_qoin_scoring_config(serialized: &str) -> Result<QoinScoringConfig, ConfigError> {
    let policy: QoinScoringConfig =
        serde_json::from_str(serialized).map_err(|_| ConfigError::InvalidQoinScoringConfig)?;
    policy
        .validate()
        .map_err(|_| ConfigError::InvalidQoinScoringConfig)?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_parser_accepts_explicit_values_and_rejects_ambiguity() {
        assert_eq!(parse_bool("FLAG", "true"), Ok(true));
        assert_eq!(parse_bool("FLAG", "NO"), Ok(false));
        assert_eq!(
            parse_bool("FLAG", "sometimes"),
            Err(ConfigError::InvalidBoolean("FLAG"))
        );
    }

    #[test]
    fn production_cannot_disable_required_mongodb() {
        assert_eq!(mongo_is_required("production", Some("false")), Ok(true));
        assert_eq!(mongo_is_required("development", Some("false")), Ok(false));
        assert_eq!(mongo_is_required("test", Some("true")), Ok(true));
    }

    #[test]
    fn pool_size_and_timeouts_are_bounded() {
        assert_eq!(parse_bounded_integer("POOL", "24", 1, 500), Ok(24));
        assert!(parse_bounded_integer("POOL", "0", 1, 500).is_err());
        assert!(parse_bounded_integer("POOL", "501", 1, 500).is_err());
        assert_eq!(
            parse_duration_ms("TIMEOUT", "10000"),
            Ok(Duration::from_secs(10))
        );
        assert!(parse_duration_ms("TIMEOUT", "99").is_err());
        assert!(parse_duration_ms("TIMEOUT", "120001").is_err());
    }

    #[test]
    fn custom_qoin_policy_is_validated_before_startup() {
        let policy = QoinScoringConfig::default();
        let serialized = serde_json::to_string(&policy).unwrap();
        assert_eq!(parse_qoin_scoring_config(&serialized).unwrap(), policy);
        assert!(parse_qoin_scoring_config("{").is_err());
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
