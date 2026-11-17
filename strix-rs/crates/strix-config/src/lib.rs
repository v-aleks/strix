//! Settings and model profiles for the Rust Strix engine.
//!
//! The Python engine resolves a `provider/model` identifier through `LiteLLM` and
//! supports nine cloud providers. The Rust port targets exactly one provider:
//! a local `llama-server` speaking the `OpenAI` chat-completions protocol. This
//! crate is therefore the whole provider abstraction surface:
//!
//! * [`Settings`] — every environment variable the Rust engine reads, with the
//!   Python variable names preserved so existing setups keep working.
//! * [`ModelProfile`] — the per-model quirks that a llama.cpp deployment
//!   exposes (tool-call id behaviour, parallel tool calls, context window).
//!
//! Anything that only existed to support a cloud provider (prompt caching,
//! reasoning effort, provider prefixes, cost tables) is deliberately absent —
//! see `CONTRACT.md` for the full mapping.

use std::collections::BTreeMap;
use std::env;

use serde::{Deserialize, Serialize};

/// Errors raised while resolving settings from the environment.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// A variable was present but could not be parsed into the expected type.
    #[error("invalid value for {name}: {reason}")]
    InvalidValue { name: String, reason: String },
    /// A required variable was missing.
    #[error("{name} is required but not set")]
    Missing { name: String },
}

/// How the provider reports tool-call identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallIdMode {
    /// The server returns a usable, unique id for every call.
    Server,
    /// The server returns an empty id; the client synthesizes `call_<uuid>`.
    #[default]
    Synthesize,
    /// The server restarts its per-turn counter, so ids collide across turns
    /// and within a turn. The client rewrites them by position.
    Positional,
}

/// Quirks of one model served by llama.cpp.
///
/// Profiles are the single place where "Gemma behaves differently from Qwen"
/// is encoded. Every field has a conservative default: when a profile is
/// missing or a field is unknown, the engine assumes the worst case observed
/// in the wild for locally served models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    /// Display name of the profile family (`gemma`, `qwen`, ...).
    pub family: String,
    /// How tool-call ids arrive from the server.
    #[serde(default)]
    pub tool_call_id_mode: ToolCallIdMode,
    /// Whether the server can emit more than one tool call per assistant turn.
    #[serde(default)]
    pub supports_parallel_tool_calls: bool,
    /// Whether the server emits `reasoning_content` alongside the answer.
    #[serde(default)]
    pub supports_reasoning: bool,
    /// Maximum tool calls accepted in a single assistant turn.
    #[serde(default = "default_max_tool_calls_per_turn")]
    pub max_tool_calls_per_turn: usize,
    /// Context window in tokens, when known.
    #[serde(default)]
    pub context_tokens: Option<u32>,
    /// Maximum tokens the server may generate per turn, when known.
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature used for scan turns.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_max_tool_calls_per_turn() -> usize {
    8
}

fn default_temperature() -> f32 {
    0.2
}

impl ModelProfile {
    /// Build a profile from a model name using the built-in family heuristics.
    ///
    /// Detection is deliberately narrow: an unknown model gets the conservative
    /// default profile, never an optimistic one.
    #[must_use]
    pub fn detect(model: &str) -> Self {
        let lowered = model.to_ascii_lowercase();
        let family = if lowered.contains("gemma") {
            "gemma"
        } else if lowered.contains("qwen") {
            "qwen"
        } else if lowered.contains("llama") {
            "llama"
        } else {
            "generic"
        };

        match family {
            // Gemma ships a conservative chat template that emits a single
            // tool call per turn and frequently leaves the call id empty.
            "gemma" => Self {
                family: family.to_owned(),
                tool_call_id_mode: ToolCallIdMode::Synthesize,
                supports_parallel_tool_calls: false,
                supports_reasoning: false,
                max_tool_calls_per_turn: 1,
                context_tokens: None,
                max_output_tokens: None,
                temperature: default_temperature(),
            },
            // Qwen3 emits parallel calls with ids that restart per turn.
            "qwen" => Self {
                family: family.to_owned(),
                tool_call_id_mode: ToolCallIdMode::Positional,
                supports_parallel_tool_calls: true,
                supports_reasoning: false,
                max_tool_calls_per_turn: 8,
                context_tokens: None,
                max_output_tokens: None,
                temperature: default_temperature(),
            },
            _ => Self {
                family: family.to_owned(),
                tool_call_id_mode: ToolCallIdMode::Synthesize,
                supports_parallel_tool_calls: false,
                supports_reasoning: false,
                max_tool_calls_per_turn: default_max_tool_calls_per_turn(),
                context_tokens: None,
                max_output_tokens: None,
                temperature: default_temperature(),
            },
        }
    }

    /// Apply a profile supplied by the operator.
    ///
    /// Only the fields present in the override are replaced; the rest keep the
    /// detected values, so a partial override never silently resets the family
    /// heuristics.
    #[must_use]
    pub fn merged_with(&self, override_profile: &ModelProfile) -> Self {
        ModelProfile {
            family: if override_profile.family.is_empty() {
                self.family.clone()
            } else {
                override_profile.family.clone()
            },
            tool_call_id_mode: override_profile.tool_call_id_mode,
            supports_parallel_tool_calls: override_profile.supports_parallel_tool_calls,
            supports_reasoning: override_profile.supports_reasoning,
            max_tool_calls_per_turn: override_profile.max_tool_calls_per_turn,
            context_tokens: override_profile.context_tokens.or(self.context_tokens),
            max_output_tokens: override_profile
                .max_output_tokens
                .or(self.max_output_tokens),
            temperature: override_profile.temperature,
        }
    }
}

/// Context-window and output limits used when the server does not report them.
pub mod limits {
    /// Fallback context window when neither profile nor server reports one.
    pub const FALLBACK_CONTEXT_TOKENS: u32 = 200_000;
    /// Fallback per-turn output cap.
    pub const DEFAULT_OUTPUT_TOKENS: u32 = 8_192;
    /// Characters-per-token ratio used by the tokenizer fallback.
    pub const FALLBACK_CHARS_PER_TOKEN: f64 = 3.5;
}

/// Tool-output bounds, mirroring the Python `ContextSettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSettings {
    /// Whether the engine compacts the conversation on overflow.
    pub auto_compact: bool,
    /// Tokens reserved for the model's own answer when deciding to compact.
    pub compact_buffer_tokens: u32,
    /// Tokens kept from the tail of the conversation after compaction.
    pub keep_tokens: u32,
    /// Maximum tool-output tokens handed back to the model.
    pub tool_output_max_tokens: u32,
    /// Maximum tool-output lines handed back to the model.
    pub tool_output_max_lines: u32,
    /// Maximum tool-output bytes handed back to the model.
    pub tool_output_max_bytes: u32,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            auto_compact: true,
            compact_buffer_tokens: 20_000,
            keep_tokens: 8_000,
            tool_output_max_tokens: 8_000,
            tool_output_max_lines: 2_000,
            tool_output_max_bytes: 50 * 1024,
        }
    }
}

/// Budget and turn limits.
///
/// The Python engine budgets in US dollars against `LiteLLM`'s price table. A
/// local provider costs nothing, so the Rust engine budgets in tokens and
/// keeps the dollar path only for operators who price their hardware.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetSettings {
    /// Maximum tokens the whole scan may consume. `None` means unlimited.
    pub max_budget_tokens: Option<u64>,
    /// Maximum number of model turns per agent.
    pub max_turns: u32,
    /// Fraction of the budget reserved for the root agent; sub-agents stop at
    /// `1 - reserve`.
    pub subagent_reserve: f64,
    /// Optional price per million input tokens, for dollar-denominated budgets.
    pub input_cost_per_mtok: Option<f64>,
    /// Optional price per million output tokens.
    pub output_cost_per_mtok: Option<f64>,
    /// Optional price per million cached input tokens.
    pub cached_input_cost_per_mtok: Option<f64>,
    /// Maximum budget in US dollars, when the operator prefers that unit.
    pub max_budget_usd: Option<f64>,
}

impl Default for BudgetSettings {
    fn default() -> Self {
        Self {
            max_budget_tokens: None,
            max_turns: 500,
            subagent_reserve: 0.10,
            input_cost_per_mtok: None,
            output_cost_per_mtok: None,
            cached_input_cost_per_mtok: None,
            max_budget_usd: None,
        }
    }
}

/// Provider connection settings for the single local llama.cpp server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// Base URL of the OpenAI-compatible server, including the `/v1` prefix.
    pub api_base: String,
    /// Optional API key. llama.cpp does not check it, but the field is sent
    /// when present so that a reverse proxy can require one.
    pub api_key: Option<String>,
    /// Model name as the server knows it.
    pub model: String,
    /// Extra headers sent with every request.
    pub extra_headers: BTreeMap<String, String>,
    /// Extra top-level JSON fields merged into every request body.
    pub extra_body: serde_json::Value,
    /// Whether streaming is used. Disable for servers with unreliable SSE.
    pub streaming: bool,
    /// Per-request timeout in seconds.
    pub timeout_secs: u64,
    /// Timeout for the very first streamed event.
    pub stream_first_event_timeout_secs: u64,
    /// Maximum gap between two streamed events.
    pub stream_idle_timeout_secs: u64,
    /// Maximum total duration of one streamed response.
    pub stream_total_timeout_secs: u64,
    /// Number of retries for transient provider failures.
    pub max_retries: u32,
    /// Base delay of the retry backoff, in seconds.
    pub retry_base_delay_secs: u64,
    /// Maximum delay of the retry backoff, in seconds.
    pub retry_max_delay_secs: u64,
    /// Profile override supplied by the operator, as JSON.
    pub profile_override: Option<serde_json::Value>,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            api_base: "http://127.0.0.1:8080/v1".to_owned(),
            api_key: None,
            model: String::new(),
            extra_headers: BTreeMap::new(),
            extra_body: serde_json::Value::Null,
            streaming: true,
            timeout_secs: 300,
            stream_first_event_timeout_secs: 120,
            stream_idle_timeout_secs: 300,
            stream_total_timeout_secs: 600,
            max_retries: 5,
            retry_base_delay_secs: 2,
            retry_max_delay_secs: 90,
            profile_override: None,
        }
    }
}

/// Sandbox settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSettings {
    /// Sandbox container image.
    pub image: String,
    /// Whether the Docker sandbox is mandatory. It always is in this product.
    pub sandbox_required: bool,
    /// Maximum number of images kept in a conversation.
    pub max_context_images: u32,
    /// Root of the workspace inside the container.
    pub workspace_root: String,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            image: "ghcr.io/usestrix/strix-sandbox:1.4.0".to_owned(),
            sandbox_required: true,
            max_context_images: 3,
            workspace_root: "/workspace".to_owned(),
        }
    }
}

/// Full engine settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Provider connection.
    pub provider: ProviderSettings,
    /// Context and compaction behaviour.
    pub context: ContextSettings,
    /// Budget and turn limits.
    pub budget: BudgetSettings,
    /// Sandbox configuration.
    pub runtime: RuntimeSettings,
    /// Whether the LLM dedupe pass runs on every filed finding.
    pub dedupe_enabled: bool,
    /// Whether telemetry is sent. Off by default in the Rust port.
    pub telemetry_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: ProviderSettings::default(),
            context: ContextSettings::default(),
            budget: BudgetSettings::default(),
            runtime: RuntimeSettings::default(),
            dedupe_enabled: true,
            telemetry_enabled: false,
        }
    }
}

impl Settings {
    /// Resolve settings from the process environment.
    ///
    /// Python variable names are reused so that an existing `STRIX_LLM` /
    /// `LLM_API_BASE` setup keeps working unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError::InvalidValue`] when a variable is present but
    /// malformed, and [`SettingsError::Missing`] when `STRIX_LLM` is absent.
    pub fn from_env() -> Result<Self, SettingsError> {
        let mut settings = Settings::default();

        let model = env::var("STRIX_LLM")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| SettingsError::Missing {
                name: "STRIX_LLM".to_owned(),
            })?;
        settings.provider.model = model;

        if let Some(base) = first_env(&["LLM_API_BASE", "OPENAI_BASE_URL", "OPENAI_API_BASE"]) {
            settings.provider.api_base = base;
        }
        if let Some(key) = first_env(&["LLM_API_KEY", "OPENAI_API_KEY"]) {
            settings.provider.api_key = Some(key);
        }
        if let Some(raw) = env_text("LLM_EXTRA_HEADERS") {
            settings.provider.extra_headers = parse_json_map("LLM_EXTRA_HEADERS", &raw)?;
        }
        if let Some(raw) = env_text("LLM_EXTRA_BODY") {
            settings.provider.extra_body =
                serde_json::from_str(&raw).map_err(|err| SettingsError::InvalidValue {
                    name: "LLM_EXTRA_BODY".to_owned(),
                    reason: err.to_string(),
                })?;
        }
        if let Some(value) = env_text("LLM_DISABLE_STREAMING") {
            settings.provider.streaming = !truthy(&value);
        }
        if let Some(value) = env_u64("LLM_TIMEOUT")? {
            settings.provider.timeout_secs = value;
        }
        if let Some(value) = env_u64("LLM_STREAM_FIRST_EVENT_TIMEOUT")? {
            settings.provider.stream_first_event_timeout_secs = value;
        }
        if let Some(value) = env_u64("LLM_STREAM_IDLE_TIMEOUT")? {
            settings.provider.stream_idle_timeout_secs = value;
        }
        if let Some(value) = env_u64("LLM_STREAM_TOTAL_TIMEOUT")? {
            settings.provider.stream_total_timeout_secs = value;
        }
        if let Some(value) = env_u64("STRIX_MAX_BUDGET_TOKENS")? {
            settings.budget.max_budget_tokens = Some(value);
        }
        if let Some(value) = env_f64("STRIX_MAX_BUDGET")? {
            settings.budget.max_budget_usd = Some(value);
        }
        if let Some(value) = env_u32("STRIX_MAX_TURNS")? {
            settings.budget.max_turns = value;
        }
        if let Some(value) = env_f64("STRIX_LLM_INPUT_COST_PER_MTOK")? {
            settings.budget.input_cost_per_mtok = Some(value);
        }
        if let Some(value) = env_f64("STRIX_LLM_OUTPUT_COST_PER_MTOK")? {
            settings.budget.output_cost_per_mtok = Some(value);
        }
        if let Some(value) = env_f64("STRIX_LLM_CACHED_INPUT_COST_PER_MTOK")? {
            settings.budget.cached_input_cost_per_mtok = Some(value);
        }
        if let Some(value) = env_text("STRIX_CONTEXT_AUTO_COMPACT") {
            settings.context.auto_compact = truthy(&value);
        }
        if let Some(value) = env_u32("STRIX_CONTEXT_KEEP_TOKENS")? {
            settings.context.keep_tokens = value;
        }
        if let Some(value) = env_u32("STRIX_CONTEXT_BUFFER_TOKENS")? {
            settings.context.compact_buffer_tokens = value;
        }
        if let Some(value) = env_u32("STRIX_TOOL_OUTPUT_MAX_TOKENS")? {
            settings.context.tool_output_max_tokens = value;
        }
        if let Some(value) = env_u32("STRIX_TOOL_OUTPUT_MAX_LINES")? {
            settings.context.tool_output_max_lines = value;
        }
        if let Some(value) = env_u32("STRIX_TOOL_OUTPUT_MAX_BYTES")? {
            settings.context.tool_output_max_bytes = value;
        }
        if let Some(value) = env_text("STRIX_IMAGE") {
            settings.runtime.image = value;
        }
        if let Some(value) = env_u32("STRIX_MAX_CONTEXT_IMAGES")? {
            settings.runtime.max_context_images = value;
        }
        if let Some(value) = env_text("STRIX_DEDUPE_ENABLED") {
            settings.dedupe_enabled = truthy(&value);
        }
        if let Some(value) = env_text("STRIX_TELEMETRY") {
            settings.telemetry_enabled = truthy(&value);
        }
        if let Some(value) = env_text("STRIX_MODEL_PROFILE") {
            settings.provider.profile_override = Some(parse_profile_override(&value));
        }

        Ok(settings)
    }

    /// Resolve the effective model profile.
    ///
    /// The detected family profile is the baseline; an operator override
    /// replaces the fields it declares.
    #[must_use]
    pub fn model_profile(&self) -> ModelProfile {
        let detected = ModelProfile::detect(&self.provider.model);
        let Some(override_value) = self.provider.profile_override.as_ref() else {
            return detected;
        };
        let Ok(override_profile) = serde_json::from_value::<ModelProfile>(override_value.clone())
        else {
            tracing::warn!("STRIX_MODEL_PROFILE is not a valid profile object; using detection");
            return detected;
        };
        detected.merged_with(&override_profile)
    }

    /// Context window in tokens.
    #[must_use]
    pub fn context_window(&self) -> u32 {
        self.model_profile()
            .context_tokens
            .unwrap_or(limits::FALLBACK_CONTEXT_TOKENS)
    }

    /// Maximum output tokens per turn.
    #[must_use]
    pub fn output_limit(&self) -> u32 {
        self.model_profile()
            .max_output_tokens
            .unwrap_or(limits::DEFAULT_OUTPUT_TOKENS)
    }
}

/// Read a non-empty environment variable as text.
fn env_text(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

/// Read an environment variable as an unsigned integer.
fn env_u64(name: &str) -> Result<Option<u64>, SettingsError> {
    env_text(name).map(|raw| parse_u64(name, &raw)).transpose()
}

/// Read an environment variable as a 32-bit unsigned integer.
fn env_u32(name: &str) -> Result<Option<u32>, SettingsError> {
    env_text(name)
        .map(|raw| {
            parse_u64(name, &raw).and_then(|value| {
                u32::try_from(value).map_err(|_| SettingsError::InvalidValue {
                    name: name.to_owned(),
                    reason: format!("{value} does not fit into 32 bits"),
                })
            })
        })
        .transpose()
}

/// Read an environment variable as a floating-point number.
fn env_f64(name: &str) -> Result<Option<f64>, SettingsError> {
    env_text(name).map(|raw| parse_f64(name, &raw)).transpose()
}

fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_u64(name: &str, raw: &str) -> Result<u64, SettingsError> {
    raw.trim()
        .parse::<u64>()
        .map_err(|err| SettingsError::InvalidValue {
            name: name.to_owned(),
            reason: err.to_string(),
        })
}

fn parse_f64(name: &str, raw: &str) -> Result<f64, SettingsError> {
    raw.trim()
        .parse::<f64>()
        .map_err(|err| SettingsError::InvalidValue {
            name: name.to_owned(),
            reason: err.to_string(),
        })
}

fn parse_json_map(name: &str, raw: &str) -> Result<BTreeMap<String, String>, SettingsError> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| SettingsError::InvalidValue {
            name: name.to_owned(),
            reason: err.to_string(),
        })?;
    let object = parsed
        .as_object()
        .ok_or_else(|| SettingsError::InvalidValue {
            name: name.to_owned(),
            reason: "expected a JSON object".to_owned(),
        })?;
    object
        .iter()
        .map(|(key, value)| {
            let text = match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            Ok((key.clone(), text))
        })
        .collect()
}

fn parse_profile_override(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return value;
    }
    // A bare family name selects the built-in heuristics for that family.
    let profile = ModelProfile::detect(trimmed);
    serde_json::to_value(profile).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn detects_gemma_as_single_call_without_ids() {
        let profile = ModelProfile::detect("gemma-3-27b-it");
        assert_eq!(profile.family, "gemma");
        assert_eq!(profile.tool_call_id_mode, ToolCallIdMode::Synthesize);
        assert!(!profile.supports_parallel_tool_calls);
        assert_eq!(profile.max_tool_calls_per_turn, 1);
    }

    #[test]
    fn detects_qwen_with_positional_ids() {
        let profile = ModelProfile::detect("qwen3-coder-30b");
        assert_eq!(profile.family, "qwen");
        assert_eq!(profile.tool_call_id_mode, ToolCallIdMode::Positional);
        assert!(profile.supports_parallel_tool_calls);
    }

    #[test]
    fn unknown_model_gets_conservative_defaults() {
        let profile = ModelProfile::detect("some-local-model");
        assert_eq!(profile.family, "generic");
        assert!(!profile.supports_parallel_tool_calls);
        assert_eq!(profile.tool_call_id_mode, ToolCallIdMode::Synthesize);
    }

    #[test]
    fn override_keeps_detected_context_window_when_absent() {
        let detected = ModelProfile {
            context_tokens: Some(32_768),
            ..ModelProfile::detect("qwen3")
        };
        let mut override_profile = ModelProfile::detect("qwen3");
        override_profile.context_tokens = None;
        override_profile.family = String::new();
        let merged = detected.merged_with(&override_profile);
        assert_eq!(merged.context_tokens, Some(32_768));
        assert_eq!(merged.family, "qwen");
    }

    #[test]
    fn profile_override_from_family_name() {
        let value = parse_profile_override("gemma");
        let profile = serde_json::from_value::<ModelProfile>(value).unwrap();
        assert_eq!(profile.family, "gemma");
    }

    #[test]
    fn truthy_parsing() {
        assert!(truthy("1"));
        assert!(truthy("TRUE"));
        assert!(truthy("yes"));
        assert!(!truthy("0"));
        assert!(!truthy(""));
    }
}
