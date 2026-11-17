//! Wire types of the chat-completions protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: String,
    /// The message body.
    pub content: Value,
    /// Tool calls attached to an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// The tool call this message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The tool name this message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    /// A plain text message.
    #[must_use]
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_owned(),
            content: Value::String(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// A user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text("user", content)
    }

    /// A system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text("system", content)
    }

    /// A tool-result message answering the given call.
    #[must_use]
    pub fn tool_result(call_id: &str, name: &str, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_owned(),
            content: Value::String(content.into()),
            tool_calls: None,
            tool_call_id: Some(call_id.to_owned()),
            name: Some(name.to_owned()),
        }
    }
}

/// A tool call requested by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The call identifier. May be empty — local servers often omit it.
    #[serde(default)]
    pub id: String,
    /// The tool name.
    pub function: ToolCallFunction,
}

impl ToolCall {
    /// The tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.function.name
    }

    /// The raw argument string, which may be truncated or invalid JSON.
    #[must_use]
    pub fn arguments(&self) -> &str {
        &self.function.arguments
    }
}

/// The function part of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallFunction {
    /// The tool name.
    pub name: String,
    /// The raw argument string.
    pub arguments: String,
}

/// A chat-completions request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// The model name as the server knows it.
    pub model: String,
    /// The conversation.
    pub messages: Vec<ChatMessage>,
    /// Tool definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    /// Whether the server may answer with more than one tool call.
    #[serde(default)]
    pub parallel_tool_calls: bool,
    /// Whether the response is streamed.
    #[serde(default)]
    pub stream: bool,
}

impl ChatRequest {
    /// A non-streaming request.
    #[must_use]
    pub fn new(model: &str, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.to_owned(),
            messages,
            tools: None,
            parallel_tool_calls: false,
            stream: false,
        }
    }
}

/// Token accounting reported by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt.
    #[serde(default)]
    pub prompt_tokens: u64,
    /// Tokens in the completion.
    #[serde(default)]
    pub completion_tokens: u64,
    /// Sum of both.
    #[serde(default)]
    pub total_tokens: u64,
}

impl Usage {
    /// The number of tokens a local provider bills for.
    ///
    /// A local provider has no cached-input concept, so the total is the
    /// prompt plus the completion.
    #[must_use]
    pub fn billable_tokens(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.prompt_tokens + self.completion_tokens
        }
    }
}

/// A chat-completions response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    /// The response identifier.
    #[serde(default)]
    pub id: String,
    /// The model that produced the answer.
    #[serde(default)]
    pub model: String,
    /// The choices the server returned.
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    /// Token accounting.
    #[serde(default)]
    pub usage: Usage,
}

/// One choice of a chat-completions response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChoice {
    /// The assistant message.
    pub message: ChatMessage,
    /// Why the model stopped.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

impl ChatResponse {
    /// The first choice, when the server returned one.
    #[must_use]
    pub fn first_choice(&self) -> Option<&ChatChoice> {
        self.choices.first()
    }

    /// The tool calls of the first choice.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCall] {
        self.first_choice()
            .and_then(|choice| choice.message.tool_calls.as_deref())
            .unwrap_or(&[])
    }
}
