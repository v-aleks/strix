//! OpenAI-compatible chat-completions client for the single local provider.
//!
//! Scope of the crate (phase 1):
//!
//! * request assembly against `{api_base}/chat/completions`,
//! * SSE streaming with first-event, idle-gap and total-timeout bounds,
//! * transient-failure retries with exponential backoff,
//! * tool-call hygiene — id synthesis, argument repair, per-turn call cap,
//! * token counting through the server's `/tokenize` endpoint,
//! * request logging with the same event schema the Python engine emits.
//!
//! Phase 0 ships the transport skeleton: request assembly, a non-streaming
//! call and the provider health probe. They exist so the Rust engine can be
//! exercised against the mock `llama-server` from the very first commit, and
//! so a contract break in the wire format shows up here rather than in phase 1.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod client;
pub mod types;

pub use client::{Client, ClientError};
pub use types::{ChatMessage, ChatRequest, ChatResponse, ToolCall, Usage};

/// The provider this crate speaks to.
pub const PROVIDER_NAME: &str = "llama.cpp";

/// Build the request body for a chat-completions call.
///
/// The body is deliberately plain: `llama.cpp` accepts the `OpenAI` shape, and
/// every field the Python engine used to send through `LiteLLM` but that
/// llama.cpp ignores (provider routing, reasoning effort, prompt caching)
/// is simply absent.
#[must_use]
pub fn build_chat_request(
    model: &str,
    messages: &[ChatMessage],
    tools: Option<&[Value]>,
    extra_body: &Value,
    parallel_tool_calls: bool,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "parallel_tool_calls": parallel_tool_calls,
    });

    if let Some(tools) = tools {
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            // Tool choice is never forced: a local model that must call a tool
            // tends to answer with prose instead, and a forced call produces a
            // hallucinated argument set.
            body["tool_choice"] = json!("auto");
        }
    }

    if let Value::Object(extra) = extra_body {
        if let Some(object) = body.as_object_mut() {
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
    }

    body
}

/// Timeout derived from a seconds count.
#[must_use]
pub fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// Serialize a request body for the wire.
#[must_use]
pub fn encode_body(body: &Value) -> Vec<u8> {
    serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec())
}

/// A provider-side failure the caller may retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    /// HTTP status, when the failure came with one.
    pub status: Option<u16>,
    /// Message reported by the server, when it reported one.
    pub message: String,
    /// Whether the failure is worth retrying.
    pub retryable: bool,
}

impl ProviderError {
    /// Build a retryable transport error.
    #[must_use]
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            status: None,
            message: message.into(),
            retryable: true,
        }
    }

    /// Build an error for an HTTP status.
    #[must_use]
    pub fn status(status: u16, message: impl Into<String>) -> Self {
        Self {
            status: Some(status),
            message: message.into(),
            retryable: matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
        }
    }
}
