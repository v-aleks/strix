#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

//! Integration tests over the mock `llama-server`.
//!
//! These tests pin the behaviour of the mock itself, so the Rust LLM client
//! can be developed against a stable, documented target. Every failure mode
//! the Rust client must survive has a scenario here.

use std::time::Duration;

use serde_json::{json, Value};
use strix_mock_llm::{serve_ephemeral, Scenario};

async fn post(base: &str, body: Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|err| panic!("request failed: {err}"))
}

#[tokio::test]
async fn health_and_models_are_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let client = reqwest::Client::new();

    let health = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);

    let models = client
        .get(format!("{base}/v1/models"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], "mock-model");

    let props = client
        .get(format!("{base}/v1/props"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(props["n_ctx"], 32_768);
}

#[tokio::test]
async fn non_streaming_tool_call_is_well_formed() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": false,
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["id"],
        "call_ok"
    );
}

#[tokio::test]
async fn streaming_emits_sse_and_done_marker() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.starts_with("data: "));
    assert!(text.contains("data: [DONE]"));
    assert!(text.contains("\"tool_calls\""));
}

#[tokio::test]
async fn empty_tool_call_id_is_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "tool_call_empty_id",
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["choices"][0]["message"]["tool_calls"][0]["id"], "");
}

#[tokio::test]
async fn duplicate_tool_call_ids_are_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "tool_call_duplicate_ids",
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    let calls = body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], calls[1]["id"]);
}

#[tokio::test]
async fn truncated_arguments_are_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "tool_call_truncated_arguments",
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    let arguments = body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert!(serde_json::from_str::<Value>(arguments).is_err());
}

#[tokio::test]
async fn parallel_tool_calls_are_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "tool_call_parallel",
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    let calls = body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], "0");
    assert_eq!(calls[1]["id"], "1");
}

#[tokio::test]
async fn finish_reason_length_is_served() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "finish_length",
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["choices"][0]["finish_reason"], "length");
}

#[tokio::test]
async fn context_overflow_returns_400() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "context_overflow",
        }),
    )
    .await;
    assert_eq!(response.status(), 400);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["error"]["type"], "exceed_context_size_error");
}

#[tokio::test]
async fn server_error_returns_500() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "server_error",
        }),
    )
    .await;
    assert_eq!(response.status(), 500);
}

#[tokio::test]
async fn rate_limit_returns_429() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "rate_limited",
        }),
    )
    .await;
    assert_eq!(response.status(), 429);
}

#[tokio::test]
async fn stalled_stream_never_completes() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
            "stream": true,
            "mock_scenario": "stall_after_first_chunk",
        }),
    )
    .await;
    assert_eq!(response.status(), 200);

    // The client must be able to give up: reading the stream with a timeout
    // has to fail rather than hang forever.
    let outcome = tokio::time::timeout(Duration::from_millis(500), response.text()).await;
    assert!(outcome.is_err(), "a stalled stream must not deliver a body");
}

#[tokio::test]
async fn every_scenario_name_round_trips() {
    for scenario in Scenario::all() {
        let parsed = Scenario::parse(scenario.as_str());
        assert_eq!(parsed, Some(*scenario));
    }
    assert_eq!(Scenario::parse("not-a-scenario"), None);
}

#[tokio::test]
async fn requests_are_recorded_for_inspection() {
    let (base, _handle) = serve_ephemeral("mock-model").await.unwrap();
    post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hello"}],
        }),
    )
    .await;
    // The recording state is per-server; the count is observable through a
    // second request's turn number.
    let response = post(
        &base,
        json!({
            "model": "mock-model",
            "messages": [],
        }),
    )
    .await;
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["id"], "chatcmpl-mock-2");
}
