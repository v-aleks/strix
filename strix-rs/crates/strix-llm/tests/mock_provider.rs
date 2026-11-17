#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

//! The Rust client against the mock `llama-server`.
//!
//! This is the first place where the Rust engine talks the provider protocol,
//! so the failures it must survive are exercised here rather than in phase 1.

use serde_json::json;
use strix_config::ProviderSettings;
use strix_llm::{ChatMessage, ChatRequest, Client};

fn client_for(base: &str) -> Client {
    let settings = ProviderSettings {
        api_base: format!("{base}/v1"),
        model: "mock-model".to_owned(),
        ..ProviderSettings::default()
    };
    Client::new(settings).unwrap_or_else(|err| panic!("client cannot be built: {err}"))
}

#[tokio::test]
async fn health_probe_succeeds_against_the_mock() {
    let (base, _handle) = strix_mock_llm::serve_ephemeral("mock-model").await.unwrap();
    let client = client_for(&base);
    client
        .health()
        .await
        .unwrap_or_else(|err| panic!("health probe failed: {err}"));
}

#[tokio::test]
async fn chat_completion_returns_the_tool_call() {
    let (base, _handle) = strix_mock_llm::serve_ephemeral("mock-model").await.unwrap();
    let client = client_for(&base);

    let request = ChatRequest::new("mock-model", vec![ChatMessage::user("hello")]);
    let response = client
        .chat_completions(&request)
        .await
        .unwrap_or_else(|err| panic!("call failed: {err}"));

    assert_eq!(response.choices.len(), 1);
    assert_eq!(response.tool_calls().len(), 1);
    assert_eq!(response.tool_calls()[0].name(), "think");
    assert!(response.usage.billable_tokens() > 0);
}

#[tokio::test]
async fn context_overflow_is_reported_as_a_status_error() {
    let (base, _handle) = strix_mock_llm::serve_ephemeral("mock-model").await.unwrap();
    // The scenario travels in the request body, so the client can send it
    // through its extra-body channel.
    let settings = ProviderSettings {
        api_base: format!("{base}/v1"),
        model: "mock-model".to_owned(),
        extra_body: json!({"mock_scenario": "context_overflow"}),
        ..ProviderSettings::default()
    };
    let client = Client::new(settings).unwrap();

    let request = ChatRequest::new("mock-model", vec![ChatMessage::user("hello")]);
    let error = client
        .chat_completions(&request)
        .await
        .expect_err("a context overflow must not look like a success");
    match error {
        strix_llm::ClientError::Status { status, message } => {
            assert_eq!(status, 400);
            assert!(message.contains("context size"), "message was {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn server_error_is_reported_as_a_status_error() {
    let (base, _handle) = strix_mock_llm::serve_ephemeral("mock-model").await.unwrap();
    let response = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "server_error",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
}

#[tokio::test]
async fn request_body_omits_cloud_provider_features() {
    let body = strix_llm::build_chat_request(
        "qwen3-coder-30b",
        &[ChatMessage::system("you are a pentester")],
        Some(&[json!({"type": "function", "function": {"name": "think"}})]),
        &json!({"top_p": 0.9}),
        false,
    );

    assert_eq!(body["model"], "qwen3-coder-30b");
    assert_eq!(body["parallel_tool_calls"], false);
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["top_p"], 0.9);

    // The features a local provider does not implement must not be sent.
    for absent in [
        "reasoning_effort",
        "prompt_cache_key",
        "cached_tokens",
        "provider",
        "api_key",
    ] {
        assert!(
            body.get(absent).is_none(),
            "{absent} must not be sent to a local provider"
        );
    }
}

#[tokio::test]
async fn empty_tool_call_id_survives_deserialization() {
    let (base, _handle) = strix_mock_llm::serve_ephemeral("mock-model").await.unwrap();
    let response = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mock-model",
            "messages": [],
            "mock_scenario": "tool_call_empty_id",
        }))
        .send()
        .await
        .unwrap();
    let parsed: strix_llm::ChatResponse = response.json().await.unwrap();
    assert_eq!(parsed.tool_calls().len(), 1);
    assert_eq!(parsed.tool_calls()[0].id, "");
}
