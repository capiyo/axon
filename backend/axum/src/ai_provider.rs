use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The role requested from a provider. This is metadata for routing and auditing,
/// not a user-selectable model setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRole {
    ChecklistWriter,
    Worker,
    Supervisor,
}

/// Provider-neutral request. Secrets and provider-specific headers are owned by
/// the concrete adapter and must never be placed in this value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub role: AiRole,
    pub system_prompt: String,
    pub input: String,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub provider_id: String,
    pub model_id: String,
    pub content: String,
    pub usage: AiUsage,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiProviderError {
    #[error("provider request timed out")]
    Timeout,
    #[error("provider is temporarily unavailable")]
    Unavailable,
    #[error("provider rejected the request: {0}")]
    Rejected(String),
    #[error("provider returned an invalid response")]
    InvalidResponse,
}

/// Provider boundary used by all supervisor roles. Implementations must obtain
/// credentials from server-side configuration and must not log prompt contents.
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn provider_id(&self) -> &str;

    async fn generate(&self, request: AiRequest) -> Result<AiResponse, AiProviderError>;
}

