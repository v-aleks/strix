//! Mock `llama-server` used by the Rust test suite.
//!
//! The Python engine had no test that ever talked to a real provider, so its
//! behaviour against an OpenAI-compatible server was unverified. The Rust port
//! closes that gap: this mock reproduces the quirks that locally served
//! models actually exhibit, and the Rust client is required to survive every
//! one of them.
//!
//! Scenarios are selected per request through a header (`X-Mock-Scenario`) or
//! through the `mock_scenario` field of the request body, so a single test can
//! mix scenarios across turns.
//!
//! Endpoints:
//!
//! * `POST /v1/chat/completions` — streaming and non-streaming chat completions.
//! * `GET  /v1/models` — model listing.
//! * `GET  /v1/props` — server properties (context size and friends).
//! * `POST /tokenize` — tokenizer endpoint.
//! * `GET  /health` — readiness probe.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Scenario that shapes the next response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// A single well-formed tool call with a usable id.
    ToolCallOk,
    /// A tool call whose id is an empty string.
    ToolCallEmptyId,
    /// Two tool calls sharing the same id.
    ToolCallDuplicateIds,
    /// A tool call whose arguments are a truncated JSON fragment.
    ToolCallTruncatedArguments,
    /// Two tool calls in one turn, with per-turn ids `0` and `1`.
    ToolCallParallel,
    /// A plain text answer with no tool call.
    TextOnly,
    /// A `finish_reason` of `length`, i.e. the answer was cut off.
    FinishLength,
    /// HTTP 400 with a context-window message.
    ContextOverflow,
    /// HTTP 500.
    ServerError,
    /// HTTP 429.
    RateLimited,
    /// A stream that sends the first chunk and then stalls forever.
    StallAfterFirstChunk,
}

impl Scenario {
    /// Parse a scenario name.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "tool_call_ok" => Some(Self::ToolCallOk),
            "tool_call_empty_id" => Some(Self::ToolCallEmptyId),
            "tool_call_duplicate_ids" => Some(Self::ToolCallDuplicateIds),
            "tool_call_truncated_arguments" => Some(Self::ToolCallTruncatedArguments),
            "tool_call_parallel" => Some(Self::ToolCallParallel),
            "text_only" => Some(Self::TextOnly),
            "finish_length" => Some(Self::FinishLength),
            "context_overflow" => Some(Self::ContextOverflow),
            "server_error" => Some(Self::ServerError),
            "rate_limited" => Some(Self::RateLimited),
            "stall_after_first_chunk" => Some(Self::StallAfterFirstChunk),
            _ => None,
        }
    }

    /// The canonical name of this scenario.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolCallOk => "tool_call_ok",
            Self::ToolCallEmptyId => "tool_call_empty_id",
            Self::ToolCallDuplicateIds => "tool_call_duplicate_ids",
            Self::ToolCallTruncatedArguments => "tool_call_truncated_arguments",
            Self::ToolCallParallel => "tool_call_parallel",
            Self::TextOnly => "text_only",
            Self::FinishLength => "finish_length",
            Self::ContextOverflow => "context_overflow",
            Self::ServerError => "server_error",
            Self::RateLimited => "rate_limited",
            Self::StallAfterFirstChunk => "stall_after_first_chunk",
        }
    }

    /// Every scenario the mock can produce.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::ToolCallOk,
            Self::ToolCallEmptyId,
            Self::ToolCallDuplicateIds,
            Self::ToolCallTruncatedArguments,
            Self::ToolCallParallel,
            Self::TextOnly,
            Self::FinishLength,
            Self::ContextOverflow,
            Self::ServerError,
            Self::RateLimited,
            Self::StallAfterFirstChunk,
        ]
    }
}

/// Shared state: the turn counter and the recorded requests.
#[derive(Debug, Default)]
pub struct MockState {
    requests: Mutex<Vec<Value>>,
    turn: Mutex<u64>,
}

impl MockState {
    /// Number of requests the mock has served.
    pub async fn request_count(&self) -> usize {
        self.requests.lock().await.len()
    }

    /// Every request body the mock has seen.
    pub async fn recorded_requests(&self) -> Vec<Value> {
        self.requests.lock().await.clone()
    }

    async fn next_turn(&self) -> u64 {
        let mut guard = self.turn.lock().await;
        *guard += 1;
        *guard
    }
}

/// Application state of the mock.
#[derive(Clone)]
pub struct AppState {
    state: Arc<MockState>,
    model: String,
}

impl AppState {
    /// Create a state serving the given model name.
    #[must_use]
    pub fn new(model: &str) -> Self {
        Self {
            state: Arc::new(MockState::default()),
            model: model.to_owned(),
        }
    }

    /// Access the recorded request log.
    #[must_use]
    pub fn state(&self) -> Arc<MockState> {
        Arc::clone(&self.state)
    }
}

/// Build the mock router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/chat/completions", post(chat_completions))
        .route("/v1/models", get(models))
        .route("/models", get(models))
        .route("/v1/props", get(props))
        .route("/props", get(props))
        .route("/tokenize", post(tokenize))
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .route("/", get(health))
        .with_state(state)
}

/// Start the mock server on an ephemeral port and return its base URL.
///
/// # Errors
///
/// Returns an error when the listener cannot be bound.
pub async fn serve_ephemeral(model: &str) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let state = AppState::new(model);
    let app = router(state);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://{addr}"), handle))
}

async fn health() -> impl IntoResponse {
    (axum::http::StatusCode::OK, "OK")
}

async fn models(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(json!({
        "object": "list",
        "data": [{
            "id": state.model,
            "object": "model",
            "owned_by": "mock",
        }]
    }))
}

async fn props(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(json!({
        "model_path": format!("/models/{}.gguf", state.model),
        "n_ctx": 32_768,
        "chat_template": "{{ bos }}{{ messages }}{{ eos }}",
    }))
}

async fn tokenize(
    State(state): State<AppState>,
    body: Option<axum::Json<Value>>,
) -> impl IntoResponse {
    let text = body
        .and_then(|json| {
            json.0
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let tokens = text.chars().count().div_ceil(4);
    state
        .state
        .requests
        .lock()
        .await
        .push(json!({"tokenize": text.len()}));
    axum::Json(json!({"tokens": (0..tokens).collect::<Vec<usize>>()}))
}

/// Pick the scenario for this request.
fn resolve_scenario(headers: &HeaderMap, body: &Value) -> Scenario {
    if let Some(raw) = headers.get("x-mock-scenario").and_then(|v| v.to_str().ok()) {
        if let Some(scenario) = Scenario::parse(raw) {
            return scenario;
        }
    }
    if let Some(raw) = body.get("mock_scenario").and_then(Value::as_str) {
        if let Some(scenario) = Scenario::parse(raw) {
            return scenario;
        }
    }
    Scenario::ToolCallOk
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<axum::Json<Value>>,
) -> axum::response::Response {
    let body = body.map_or_else(|| json!({}), |json| json.0);
    let scenario = resolve_scenario(&headers, &body);
    let turn = state.state.next_turn().await;
    state
        .state
        .requests
        .lock()
        .await
        .push(json!({"turn": turn, "scenario": scenario.as_str(), "body": body}));

    match scenario {
        Scenario::ContextOverflow => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(json!({
                    "error": {
                        "message": "the request exceeds the available context size (32768 tokens)",
                        "type": "exceed_context_size_error",
                        "code": 400,
                    }
                })),
            )
                .into_response();
        }
        Scenario::ServerError => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": {"message": "internal server error", "code": 500}})),
            )
                .into_response();
        }
        Scenario::RateLimited => {
            return (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                axum::Json(json!({"error": {"message": "rate limit exceeded", "code": 429}})),
            )
                .into_response();
        }
        _ => {}
    }

    let streaming = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    if streaming {
        stream_response(&state.model, scenario, turn)
    } else {
        non_streaming_response(&state.model, scenario, turn)
    }
}

/// Tool calls the mock emits per scenario.
fn tool_calls(scenario: Scenario) -> Vec<Value> {
    match scenario {
        Scenario::ToolCallEmptyId => vec![json!({
            "index": 0,
            "id": "",
            "type": "function",
            "function": {"name": "think", "arguments": "{\"thought\": \"check the target\"}"}
        })],
        Scenario::ToolCallDuplicateIds => vec![
            json!({
                "index": 0,
                "id": "call_duplicate",
                "type": "function",
                "function": {"name": "think", "arguments": "{\"thought\": \"first\"}"}
            }),
            json!({
                "index": 1,
                "id": "call_duplicate",
                "type": "function",
                "function": {"name": "think", "arguments": "{\"thought\": \"second\"}"}
            }),
        ],
        Scenario::ToolCallTruncatedArguments => vec![json!({
            "index": 0,
            "id": "call_truncated",
            "type": "function",
            "function": {"name": "create_todo", "arguments": "{\"todos\": [{\"title\": \"half a call"}
        })],
        Scenario::ToolCallParallel => vec![
            json!({
                "index": 0,
                "id": "0",
                "type": "function",
                "function": {"name": "think", "arguments": "{\"thought\": \"parallel one\"}"}
            }),
            json!({
                "index": 1,
                "id": "1",
                "type": "function",
                "function": {"name": "think", "arguments": "{\"thought\": \"parallel two\"}"}
            }),
        ],
        _ => vec![json!({
            "index": 0,
            "id": "call_ok",
            "type": "function",
            "function": {"name": "think", "arguments": "{\"thought\": \"probing\"}"}
        })],
    }
}

fn finish_reason(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::TextOnly => "stop",
        Scenario::FinishLength => "length",
        _ => "tool_calls",
    }
}

fn non_streaming_response(model: &str, scenario: Scenario, turn: u64) -> axum::response::Response {
    let mut message = json!({"role": "assistant", "content": ""});
    let calls = tool_calls(scenario);
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
    }
    if scenario == Scenario::TextOnly {
        message["content"] = json!("No tool call needed; the answer is plain text.");
    }

    let payload = json!({
        "id": format!("chatcmpl-mock-{turn}"),
        "object": "chat.completion",
        "created": 1_700_000_000 + turn,
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason(scenario),
        }],
        "usage": {
            "prompt_tokens": 100 + turn,
            "completion_tokens": 20,
            "total_tokens": 120 + turn,
        }
    });

    (axum::http::StatusCode::OK, axum::Json(payload)).into_response()
}

fn stream_response(model: &str, scenario: Scenario, turn: u64) -> axum::response::Response {
    let model = model.to_owned();
    let stream = async_stream::stream! {
        let id = format!("chatcmpl-mock-{turn}");
        let mut calls = tool_calls(scenario);
        let finish = finish_reason(scenario);

        let first = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": 1_700_000_000 + turn,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": ""},
                "finish_reason": Value::Null,
            }]
        });
        yield Ok::<String, std::io::Error>(format!("data: {first}\n\n"));

        if scenario == Scenario::StallAfterFirstChunk {
            // Never send anything else: the client must hit its idle timeout.
            tokio::time::sleep(Duration::from_secs(3_600)).await;
            return;
        }

        if scenario == Scenario::TextOnly {
            let delta = json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": 1_700_000_000 + turn,
                "model": "mock-model",
                "choices": [{
                    "index": 0,
                    "delta": {"content": "No tool call needed; the answer is plain text."},
                    "finish_reason": Value::Null,
                }]
            });
            yield Ok(format!("data: {delta}\n\n"));
        } else {
            for call in calls.drain(..) {
                let delta = json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": 1_700_000_000 + turn,
                    "model": "mock-model",
                    "choices": [{
                        "index": call.get("index").cloned().unwrap_or(json!(0)),
                        "delta": {"tool_calls": [call]},
                        "finish_reason": Value::Null,
                    }]
                });
                yield Ok(format!("data: {delta}\n\n"));
            }
        }

        let final_chunk = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": 1_700_000_000 + turn,
            "model": "mock-model",
            "choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
            "usage": {
                "prompt_tokens": 100 + turn,
                "completion_tokens": 20,
                "total_tokens": 120 + turn,
            }
        });
        yield Ok(format!("data: {final_chunk}\n\n"));
        yield Ok("data: [DONE]\n\n".to_owned());
    };

    let body = axum::body::Body::from_stream(stream);
    let headers = [
        (axum::http::header::CONTENT_TYPE, "text/event-stream"),
        (axum::http::header::CACHE_CONTROL, "no-cache"),
    ];
    let mut response = axum::response::Response::new(body);
    for (name, value) in headers {
        if let Ok(header_value) = value.parse() {
            response.headers_mut().insert(name, header_value);
        }
    }
    response
}
