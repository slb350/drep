//! Named LLM providers for `drep init`.
//!
//! A preset stops a user having to know the OpenAI-compatible protocol - they
//! pick "OpenRouter", not "openai-compatible with base URL
//! `https://openrouter.ai/api/v1`". Every cloud option here speaks that
//! protocol; the difference between them is an endpoint, a model, and which
//! environment variable holds the key.
//!
//! Model defaults are the one thing here that goes stale, which is why the
//! wizard no longer relies on them: it asks the endpoint what it serves and
//! uses the default only to *preselect* an entry (see [`crate::llm::models`]).
//! A default the endpoint no longer offers is called out at the prompt rather
//! than silently replaced, so a stale one here is visible instead of inherited.
//! They remain the answer for `--provider` runs, which have no prompt.
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
    /// Wire protocol, or `None` for the default (`openai`). Written to the file
    /// only when set, so an OpenAI-compatible block keeps the shape it has had
    /// since 2.0 and nothing has to be migrated.
    pub protocol: Option<&'static str>,
    /// Where the user gets a key, or `None` when none is needed.
    ///
    /// Shown by the wizard at the moment the provider is chosen, because that
    /// is when the answer is wanted. Verified live rather than recalled; a
    /// wrong link here is worse than no link, since it sends someone to the
    /// metered console of a provider whose subscription key lives elsewhere.
    pub key_url: Option<&'static str>,
    /// Completion ceiling, or `None` to send none.
    ///
    /// Normally `None`: an unset cap is what stops a reasoning model being
    /// truncated mid-thought, and inventing one per provider is the coupling
    /// 2.0 removed. The exception is an endpoint that *requires* the field -
    /// `api.kimi.com/coding/v1` answers a bare `invalid_request_error` 400
    /// without it, verified against the live endpoint - where the value is set
    /// to the model's own output limit, so it is a required field rather than a
    /// ceiling anyone will hit.
    pub max_tokens: Option<u32>,
    /// Sampling temperature, or `None` to send none at all.
    ///
    /// Set per preset rather than globally because it is a property of the
    /// *model*: `k3` and `gpt-5.6-sol` reject the parameter outright, and a 400
    /// neither fails over nor retries. A preset that guessed would be the
    /// difference between a provider that works and one that never answers.
    pub temperature: Option<f32>,
}

/// Every preset, in the order the wizard should offer them.
///
/// Order matters - it is what `drep init`'s `--help` and the `--provider`
/// completions show, and it is what the tests assert.
pub static PRESETS: &[&LlmPreset] = &[&LOCAL, &OPENROUTER, &ZAI, &MINIMAX, &KIMI, &OPENAI, &CUSTOM];

/// LM Studio, Ollama or llama.cpp on this machine. No key, no cost.
pub static LOCAL: LlmPreset = LlmPreset {
    key: "local",
    display_name: "Local model",
    description: "LM Studio, Ollama or llama.cpp on this machine. No key, no cost.",
    endpoint: Some("http://localhost:1234/v1"),
    default_model: Some("qwen3-30b-a3b"),
    key_url: None,
    api_key_env: None,
    timeout_secs: None,
    protocol: None,
    max_tokens: None,
    temperature: Some(0.2),
};

/// One key for many providers. Good default for cloud analysis.
pub static OPENROUTER: LlmPreset = LlmPreset {
    key: "openrouter",
    display_name: "OpenRouter",
    description: "One key for many providers. Good default for cloud analysis.",
    endpoint: Some("https://openrouter.ai/api/v1"),
    default_model: Some("deepseek/deepseek-v4-pro-0813"),
    key_url: Some("https://openrouter.ai/keys"),
    api_key_env: Some("OPENROUTER_API_KEY"),
    timeout_secs: Some(1800),
    protocol: None,
    max_tokens: None,
    temperature: Some(0.2),
};

/// Directly against the OpenAI API.
pub static OPENAI: LlmPreset = LlmPreset {
    key: "openai",
    display_name: "OpenAI",
    description: "Directly against the OpenAI API.",
    endpoint: Some("https://api.openai.com/v1"),
    default_model: Some("gpt-5.6-sol"),
    key_url: Some("https://platform.openai.com/api-keys"),
    api_key_env: Some("OPENAI_API_KEY"),
    timeout_secs: Some(1800),
    protocol: None,
    max_tokens: None,
    // gpt-5.6-sol rejects `temperature` outright, so none is sent.
    temperature: None,
};

/// z.ai's GLM Coding Plan. OpenAI-compatible, and accepts a temperature.
pub static ZAI: LlmPreset = LlmPreset {
    key: "zai",
    display_name: "z.ai GLM Coding Plan",
    description: "GLM models on a coding-plan subscription. OpenAI-compatible.",
    endpoint: Some("https://api.z.ai/api/coding/paas/v4"),
    default_model: Some("glm-5.3"),
    key_url: Some("https://z.ai/manage-apikey/apikey-list"),
    api_key_env: Some("ZAI_API_KEY"),
    timeout_secs: Some(1800),
    protocol: None,
    max_tokens: None,
    temperature: Some(0.2),
};

/// MiniMax's Token Plan, over its Anthropic-compatible endpoint.
///
/// MiniMax publishes both `/v1` (OpenAI-compatible) and `/anthropic/v1`. The
/// Anthropic one is the preset because it is the only one that separates the
/// reasoning channel: over `/v1` the M-series returns its whole trace inline in
/// `message.content` wrapped in `<think>` tags, which drep has to strip back
/// out. Both work; one of them needs no repair.
pub static MINIMAX: LlmPreset = LlmPreset {
    key: "minimax",
    display_name: "MiniMax Token Plan",
    description: "MiniMax M-series on a token-plan subscription. Anthropic protocol.",
    endpoint: Some("https://api.minimax.io/anthropic/v1"),
    default_model: Some("MiniMax-M3"),
    key_url: Some("https://platform.minimax.io/user-center/payment/token-plan"),
    api_key_env: Some("MINIMAX_API_KEY"),
    timeout_secs: Some(1800),
    protocol: Some("anthropic"),
    max_tokens: None,
    temperature: Some(0.2),
};

/// Moonshot's Kimi for Coding plan. Anthropic protocol, and no temperature.
pub static KIMI: LlmPreset = LlmPreset {
    key: "kimi",
    display_name: "Kimi for Coding",
    description: "Moonshot's k3 on a coding-plan subscription. Anthropic protocol.",
    endpoint: Some("https://api.kimi.com/coding/v1"),
    default_model: Some("k3"),
    key_url: Some("https://www.kimi.com/code"),
    api_key_env: Some("KIMI_API_KEY"),
    timeout_secs: Some(1800),
    protocol: Some("anthropic"),
    // Required by this endpoint, not a ceiling: without it the request is refused
    // with a bare `invalid_request_error` 400 that names no field. Set below the
    // model's window with room to spare, so the required field can never be what
    // truncates an answer.
    max_tokens: Some(200_000),
    // k3 answers `only temperature 1 is allowed for this model` with a 400, which
    // neither fails over nor retries. Sending none is the only value that works.
    temperature: None,
};

/// Any other OpenAI-compatible endpoint.
pub static CUSTOM: LlmPreset = LlmPreset {
    key: "custom",
    display_name: "Custom endpoint",
    description: "Any other OpenAI-compatible endpoint.",
    endpoint: None,
    default_model: None,
    key_url: None,
    api_key_env: Some("LLM_API_KEY"),
    timeout_secs: None,
    protocol: None,
    max_tokens: None,
    temperature: Some(0.2),
};

impl LlmPreset {
    /// The wire protocol this preset's endpoint speaks.
    ///
    /// The table stores a string because that is what `drep.toml` carries and
    /// what `config_file::render_one` writes. Parsing it here rather than at
    /// each use means a typo in the table is a panic in the preset tests rather
    /// than an `unwrap_or_default()` that silently builds an OpenAI client for
    /// an Anthropic endpoint.
    pub fn protocol(&self) -> open_agent::ApiProtocol {
        crate::config::parse_protocol(self.protocol)
            .unwrap_or_else(|| panic!("preset `{}` names an unknown protocol", self.key))
    }
}

/// Look up a preset by its key.
pub fn preset(key: &str) -> Option<&'static LlmPreset> {
    PRESETS.iter().copied().find(|p| p.key == key)
}

/// Every preset key, in [`PRESETS`] order.
pub fn preset_keys() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.key).collect()
}
