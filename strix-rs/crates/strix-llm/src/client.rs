//! HTTP transport for the local provider.

use serde_json::Value;
use strix_config::ProviderSettings;

use crate::types::{ChatRequest, ChatResponse};
use crate::{build_chat_request, encode_body, ProviderError};

/// Failures of the provider transport.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The server could not be reached, or the connection broke.
    #[error("provider transport failure: {0}")]
    Transport(String),
    /// The server answered with an error status.
    #[error("provider returned {status}: {message}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// The message the server reported.
        message: String,
    },
    /// The server answered with something that is not a chat completion.
    #[error("provider returned an unusable payload: {0}")]
    Malformed(String),
}

impl From<ProviderError> for ClientError {
    fn from(error: ProviderError) -> Self {
        match error.status {
            Some(status) => Self::Status {
                status,
                message: error.message,
            },
            None => Self::Transport(error.message),
        }
    }
}

/// A client for one local provider.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    settings: ProviderSettings,
}

impl Client {
    /// Build a client for the given provider settings.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] when the HTTP client cannot be built,
    /// which happens only for an invalid TLS configuration.
    pub fn new(settings: ProviderSettings) -> Result<Self, ClientError> {
        let timeout = std::time::Duration::from_secs(settings.timeout_secs.max(1));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| ClientError::Transport(err.to_string()))?;
        Ok(Self { http, settings })
    }

    /// The provider settings this client uses.
    #[must_use]
    pub fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    /// The URL of the chat-completions endpoint.
    #[must_use]
    pub fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.settings.api_base.trim_end_matches('/')
        )
    }

    /// Send a chat-completions request and read the whole answer.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] when the request fails,
    /// [`ClientError::Status`] when the server reports an error, and
    /// [`ClientError::Malformed`] when the payload is not a chat completion.
    pub async fn chat_completions(
        &self,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ClientError> {
        let body = build_chat_request(
            &request.model,
            &request.messages,
            request.tools.as_deref(),
            &self.settings.extra_body,
            request.parallel_tool_calls,
        );

        let mut builder = self
            .http
            .post(self.chat_completions_url())
            .header("content-type", "application/json")
            .body(encode_body(&body));

        if let Some(key) = self.settings.api_key.as_ref() {
            builder = builder.bearer_auth(key);
        }
        for (name, value) in &self.settings.extra_headers {
            builder = builder.header(name, value);
        }

        let response = builder
            .send()
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))?;

        if !status.is_success() {
            return Err(ClientError::Status {
                status: status.as_u16(),
                message: extract_error_message(&text),
            });
        }

        serde_json::from_str::<ChatResponse>(&text)
            .map_err(|err| ClientError::Malformed(err.to_string()))
    }

    /// Probe the provider's health endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] when the probe fails and
    /// [`ClientError::Status`] when the provider reports an error status.
    pub async fn health(&self) -> Result<(), ClientError> {
        let url = format!("{}/health", self.settings.api_base.trim_end_matches('/'));
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|err| ClientError::Transport(err.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Status {
                status: response.status().as_u16(),
                message: "health probe failed".to_owned(),
            })
        }
    }
}

/// Extract the error message from an error payload.
///
/// llama.cpp reports errors as `{"error": {"message": "..."}}`, while some
/// proxies report a bare string; both have to yield something readable.
#[must_use]
pub fn extract_error_message(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.trim().to_owned();
    };
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .map_or_else(|| text.trim().to_owned(), std::borrow::ToOwned::to_owned)
}
