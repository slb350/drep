//! Named LLM providers for `drep init`.
//!
//! A preset stops a user having to know the OpenAI-compatible protocol - they
//! pick "OpenRouter", not "openai-compatible with base URL
//! `https://openrouter.ai/api/v1`". Every cloud option here speaks that
//! protocol; the difference between them is an endpoint, a model, and which
//! environment variable holds the key.
//!
//! Model defaults are the one thing here that goes stale. They are starting
//! points the wizard offers, always editable at the prompt, and never silently
//! imposed.
//!
//! **`max_tokens` is deliberately absent.** The Python presets set it to
//! 100,000 for reasoning models. In 2.0 no cap is sent unless the user sets
//! one - see the `max_tokens` note in [`crate::config`]. A preset that set one
//! would reintroduce exactly the coupling that was removed.

/// One named way to reach a model.
#[derive(Debug, Clone, Copy)]
pub struct LlmPreset {
    /// The name clap and `init` accept on the command line.
    pub key: &'static str,
    /// What the wizard shows.
    pub display_name: &'static str,
    /// One line on when to pick it.
    pub description: &'static str,
    /// Base URL, or `None` if the user must supply one.
    pub endpoint: Option<&'static str>,
    /// Starting point for the model prompt, or `None` if the user must supply.
    pub default_model: Option<&'static str>,
    /// Environment variable holding the key. Only the *name* is written to
    /// `drep.toml` - the file is meant to be committed.
    pub api_key_env: Option<&'static str>,
    /// Request timeout. `None` inherits `LlmConfig`'s default of 60s.
    pub timeout_secs: Option<u64>,
}

/// Every preset, in the order the wizard should offer them.
///
/// Order matters - it is what `drep init`'s `--help` and the `--provider`
/// completions show, and it is what the tests assert.
pub static PRESETS: &[&LlmPreset] = &[&LOCAL, &OPENROUTER, &OPENAI, &CUSTOM];

/// LM Studio, Ollama or llama.cpp on this machine. No key, no cost.
pub static LOCAL: LlmPreset = LlmPreset {
    key: "local",
    display_name: "Local model",
    description: "LM Studio, Ollama or llama.cpp on this machine. No key, no cost.",
    endpoint: Some("http://localhost:1234/v1"),
    default_model: Some("qwen3-30b-a3b"),
    api_key_env: None,
    timeout_secs: None,
};

/// One key for many providers. Good default for cloud analysis.
pub static OPENROUTER: LlmPreset = LlmPreset {
    key: "openrouter",
    display_name: "OpenRouter",
    description: "One key for many providers. Good default for cloud analysis.",
    endpoint: Some("https://openrouter.ai/api/v1"),
    default_model: Some("deepseek/deepseek-v4-pro-0813"),
    api_key_env: Some("OPENROUTER_API_KEY"),
    timeout_secs: Some(1800),
};

/// Directly against the OpenAI API.
pub static OPENAI: LlmPreset = LlmPreset {
    key: "openai",
    display_name: "OpenAI",
    description: "Directly against the OpenAI API.",
    endpoint: Some("https://api.openai.com/v1"),
    default_model: Some("gpt-5.6-sol"),
    api_key_env: Some("OPENAI_API_KEY"),
    timeout_secs: Some(1800),
};

/// Any other OpenAI-compatible endpoint.
pub static CUSTOM: LlmPreset = LlmPreset {
    key: "custom",
    display_name: "Custom endpoint",
    description: "Any other OpenAI-compatible endpoint.",
    endpoint: None,
    default_model: None,
    api_key_env: Some("LLM_API_KEY"),
    timeout_secs: None,
};

/// Look up a preset by its key.
pub fn preset(key: &str) -> Option<&'static LlmPreset> {
    PRESETS.iter().copied().find(|p| p.key == key)
}

/// Every preset key, in [`PRESETS`] order.
pub fn preset_keys() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.key).collect()
}
