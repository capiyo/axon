//! Server-side agent provider adapter.
//!
//! Providers are configured only through environment variables. Credentials are
//! never stored in agent records, returned to clients, or written to audit logs.
//! The adapter implements the OpenAI Chat Completions wire format; compatible
//! gateways can be used for hosted or self-managed agents.
use reqwest::{header, Url};
use serde::Deserialize;
use serde_json::json;
use std::{env, time::Duration};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error("agent provider configuration is incomplete for '{0}'")]
    MissingConfiguration(String),
    #[error("invalid agent provider endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("agent provider request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("agent provider returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("agent provider returned no assistant content")]
    EmptyResponse,
    #[error("agent provider response was invalid: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, Clone)]
pub struct AgentRuntime {
    client: reqwest::Client,
    endpoint: Url,
    api_key: String,
    model: String,
    provider: String,
    anthropic: bool,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Debug, Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    text: Option<String>,
}

impl AgentRuntime {
    /// Load a provider's credentials from AXON_AGENT_<PROVIDER>_*.
    /// Provider names are normalized to uppercase ASCII alphanumeric slugs.
    pub fn from_environment(provider: &str) -> Result<Self, AgentRuntimeError> {
        let slug = provider
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect::<String>();
        if slug.is_empty() {
            return Err(AgentRuntimeError::MissingConfiguration(provider.into()));
        }
        let prefix = format!("AXON_AGENT_{slug}");
        let base_url = env::var(format!("{prefix}_BASE_URL"))
            .map_err(|_| AgentRuntimeError::MissingConfiguration(provider.into()))?;
        let api_key = env::var(format!("{prefix}_API_KEY"))
            .map_err(|_| AgentRuntimeError::MissingConfiguration(provider.into()))?;
        let model = env::var(format!("{prefix}_MODEL"))
            .map_err(|_| AgentRuntimeError::MissingConfiguration(provider.into()))?;
        if api_key.trim().is_empty() || model.trim().is_empty() {
            return Err(AgentRuntimeError::MissingConfiguration(provider.into()));
        }

        let anthropic = provider.to_ascii_lowercase().contains("claude")
            || provider.to_ascii_lowercase().contains("anthropic");
        let mut endpoint = Url::parse(&base_url)
            .map_err(|error| AgentRuntimeError::InvalidEndpoint(error.to_string()))?;
        if !matches!(endpoint.scheme(), "https" | "http") || endpoint.host_str().is_none() {
            return Err(AgentRuntimeError::InvalidEndpoint(
                "only absolute HTTP(S) endpoints are supported".into(),
            ));
        }
        let path = endpoint.path().trim_end_matches('/');
        endpoint.set_path(&format!(
            "{path}/{}",
            if anthropic { "messages" } else { "chat/completions" }
        ));
        endpoint.set_query(None);
        endpoint.set_fragment(None);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            model,
            provider: provider.to_owned(),
            anthropic,
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Execute one bounded task. The caller handles authorization, memory
    /// selection, persistence, retries, and task lifecycle transitions.
    pub async fn execute(
        &self,
        system_context: &str,
        objective: &str,
    ) -> Result<String, AgentRuntimeError> {
        let request = self.client.post(self.endpoint.clone());
        let response = if self.anthropic {
            request
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header(header::CONTENT_TYPE, "application/json")
                .json(&json!({
                    "model": self.model,
                    "max_tokens": 8192,
                    "system": system_context,
                    "messages": [{"role": "user", "content": objective}]
                }))
                .send()
                .await?
        } else {
            request
                .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
                .header(header::CONTENT_TYPE, "application/json")
                .json(&json!({
                    "model": self.model,
                    "temperature": 0.2,
                    "messages": [
                        {"role": "system", "content": system_context},
                        {"role": "user", "content": objective}
                    ]
                }))
                .send()
                .await?
        };
        if !response.status().is_success() {
            return Err(AgentRuntimeError::HttpStatus(response.status()));
        }
        let content = if self.anthropic {
            let body: AnthropicResponse = response
                .json()
                .await
                .map_err(|error| AgentRuntimeError::InvalidResponse(error.to_string()))?;
            body.content
                .into_iter()
                .filter_map(|block| block.text)
                .collect::<Vec<_>>()
                .join("\\n")
        } else {
            let body: CompletionResponse = response
                .json()
                .await
                .map_err(|error| AgentRuntimeError::InvalidResponse(error.to_string()))?;
            body.choices
                .into_iter()
                .next()
                .and_then(|choice| choice.message.content)
                .unwrap_or_default()
        };
        let content = content.trim().to_owned();
        if content.is_empty() {
            return Err(AgentRuntimeError::EmptyResponse);
        }
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_slug_is_safe_and_stable() {
        let slug = "open-ai/gpt-4.1"
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect::<String>();
        assert_eq!(slug, "OPENAIGPT41");
    }

    #[test]
    fn completion_response_extracts_assistant_content() {
        let parsed: CompletionResponse = serde_json::from_value(json!({
            "choices": [{"message": {"content": "  completed  "}}]
        }))
        .unwrap();
        assert_eq!(
            parsed.choices[0].message.content.as_deref(),
            Some("  completed  ")
        );
    }
}
