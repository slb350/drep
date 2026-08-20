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
//! would reintroduce exactly the coupling that was removed. The one exception
//! is an endpoint that *refuses a request without the field*, where the preset
//! records that requirement and a value to fall back on.
//!
//! **`temperature` and `max_tokens` are properties of a model, not a
//! provider**, so what a preset holds for them is a starting point rather than
//! an answer. [`LlmPreset::quirks`] turns the pair into a
//! [`Quirks`](crate::llm::quirks::Quirks), which
//! [`quirks::resolve`](crate::llm::quirks::resolve) then narrows against the
//! model the user actually chose. The preset's values are what a run falls back
//! to whenever the registry cannot name that model - which is every offline
//! run, every `--provider` run, and every model released since the cache was
//! written.

use crate::config::ReasoningEffort;

/// HTTP-specific preset fields.
#[derive(Debug)]
pub struct HttpPreset {
    pub endpoint: Option<&'static str>,
    pub api_key_env: Option<&'static str>,
    pub protocol: Option<&'static str>,
    pub key_url: Option<&'static str>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// Codex-specific preset fields.
#[derive(Debug)]
pub struct CodexPreset {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_concurrent: usize,
}

/// Backend-specific fields that cannot be combined across execution paths.
#[derive(Debug)]
pub enum PresetBackend {
    Http(HttpPreset),
    Codex(CodexPreset),
}

/// One named way to reach a model.
#[derive(Debug)]
pub struct LlmPreset {
    /// The name clap and `init` accept on the command line.
    pub key: &'static str,
    /// What the wizard shows.
    pub display_name: &'static str,
    /// One line on when to pick it.
    pub description: &'static str,
    /// Direct HTTP or the separately installed Codex CLI, with only the fields
    /// that backend can use.
    pub backend: PresetBackend,
    /// Starting point for the model prompt, or `None` if the user must supply.
    pub default_model: Option<&'static str>,
    /// Request timeout. `None` inherits `LlmConfig`'s default of 60s.
    pub timeout_secs: Option<u64>,
}

/// Every preset, in the order the wizard should offer them.
///
/// Order matters - it is what `drep init`'s `--help` and the `--provider`
/// completions show, and it is what the tests assert.
pub static PRESETS: &[&LlmPreset] = &[
    &LOCAL,
    &OPENROUTER,
    &ZAI,
    &MINIMAX,
    &KIMI,
    &OPENAI,
    &CODEX,
    &CUSTOM,
];

/// LM Studio, Ollama or llama.cpp on this machine. No key, no cost.
pub static LOCAL: LlmPreset = LlmPreset {
    key: "local",
    display_name: "Local model",
    description: "LM Studio, Ollama or llama.cpp on this machine. No key, no cost.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("http://localhost:1234/v1"),
        key_url: None,
        api_key_env: None,
        protocol: None,
        max_tokens: None,
        temperature: Some(0.2),
    }),
    default_model: Some("qwen3-30b-a3b"),
    timeout_secs: None,
};

/// One key for many providers. Good default for cloud analysis.
pub static OPENROUTER: LlmPreset = LlmPreset {
    key: "openrouter",
    display_name: "OpenRouter",
    description: "One key for many providers. Good default for cloud analysis.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("https://openrouter.ai/api/v1"),
        key_url: Some("https://openrouter.ai/keys"),
        api_key_env: Some("OPENROUTER_API_KEY"),
        protocol: None,
        max_tokens: None,
        temperature: Some(0.2),
    }),
    default_model: Some("deepseek/deepseek-v4-pro-0813"),
    timeout_secs: Some(1800),
};

/// Directly against the OpenAI API.
pub static OPENAI: LlmPreset = LlmPreset {
    key: "openai",
    display_name: "OpenAI API",
    description: "Directly against the OpenAI API.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("https://api.openai.com/v1"),
        key_url: Some("https://platform.openai.com/api-keys"),
        api_key_env: Some("OPENAI_API_KEY"),
        protocol: None,
        max_tokens: None,
        // gpt-5.6-sol rejects `temperature` outright, so none is sent.
        temperature: None,
    }),
    default_model: Some("gpt-5.6-sol"),
    timeout_secs: Some(1800),
};

/// The installed Codex CLI using the user's ChatGPT/Codex subscription.
pub static CODEX: LlmPreset = LlmPreset {
    key: "codex",
    display_name: "ChatGPT / Codex subscription",
    description: "Codex CLI with ChatGPT-managed authentication; no API billing.",
    backend: PresetBackend::Codex(CodexPreset {
        reasoning_effort: Some(ReasoningEffort::High),
        max_concurrent: 1,
    }),
    default_model: Some("gpt-5.6-sol"),
    timeout_secs: Some(1800),
};

/// z.ai's GLM Coding Plan. OpenAI-compatible, and accepts a temperature.
pub static ZAI: LlmPreset = LlmPreset {
    key: "zai",
    display_name: "z.ai GLM Coding Plan",
    description: "GLM models on a coding-plan subscription. OpenAI-compatible.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("https://api.z.ai/api/coding/paas/v4"),
        key_url: Some("https://z.ai/manage-apikey/apikey-list"),
        api_key_env: Some("ZAI_API_KEY"),
        protocol: None,
        max_tokens: None,
        temperature: Some(0.2),
    }),
    default_model: Some("glm-5.3"),
    timeout_secs: Some(1800),
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
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("https://api.minimax.io/anthropic/v1"),
        key_url: Some("https://platform.minimax.io/user-center/payment/token-plan"),
        api_key_env: Some("MINIMAX_API_KEY"),
        protocol: Some("anthropic"),
        max_tokens: None,
        temperature: Some(0.2),
    }),
    default_model: Some("MiniMax-M3"),
    timeout_secs: Some(1800),
};

/// Moonshot's Kimi for Coding plan. Anthropic protocol, and no temperature.
pub static KIMI: LlmPreset = LlmPreset {
    key: "kimi",
    display_name: "Kimi for Coding",
    description: "Moonshot's k3 on a coding-plan subscription. Anthropic protocol.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: Some("https://api.kimi.com/coding/v1"),
        key_url: Some("https://www.kimi.com/code"),
        api_key_env: Some("KIMI_API_KEY"),
        protocol: Some("anthropic"),
        // Required by this endpoint, not a ceiling: without it the request is refused
        // with a bare `invalid_request_error` 400 that names no field. This value is
        // only the fallback for a model the quirks registry cannot name; for one it
        // can, the model's own published output limit is written instead. Verified
        // accepted by the live endpoint, which is what a fallback has to be.
        max_tokens: Some(200_000),
        // k3 answers `only temperature 1 is allowed for this model` with a 400, which
        // neither fails over nor retries. Sending none is the only value that works.
        temperature: None,
    }),
    default_model: Some("k3"),
    timeout_secs: Some(1800),
};

/// Any other OpenAI-compatible endpoint.
pub static CUSTOM: LlmPreset = LlmPreset {
    key: "custom",
    display_name: "Custom endpoint",
    description: "Any other OpenAI-compatible endpoint.",
    backend: PresetBackend::Http(HttpPreset {
        endpoint: None,
        key_url: None,
        api_key_env: Some("LLM_API_KEY"),
        protocol: None,
        max_tokens: None,
        temperature: Some(0.2),
    }),
    default_model: None,
    timeout_secs: None,
};

impl LlmPreset {
    #[cfg(test)]
    pub fn backend_kind(&self) -> crate::config::BackendKind {
        match self.backend {
            PresetBackend::Http(_) => crate::config::BackendKind::Http,
            PresetBackend::Codex(_) => crate::config::BackendKind::Codex,
        }
    }

    pub fn http(&self) -> Option<&HttpPreset> {
        match &self.backend {
            PresetBackend::Http(http) => Some(http),
            PresetBackend::Codex(_) => None,
        }
    }

    #[cfg(test)]
    pub fn codex(&self) -> Option<&CodexPreset> {
        match &self.backend {
            PresetBackend::Codex(codex) => Some(codex),
            PresetBackend::Http(_) => None,
        }
    }

    pub fn endpoint(&self) -> Option<&'static str> {
        self.http().and_then(|http| http.endpoint)
    }

    pub fn api_key_env(&self) -> Option<&'static str> {
        self.http().and_then(|http| http.api_key_env)
    }

    pub fn key_url(&self) -> Option<&'static str> {
        self.http().and_then(|http| http.key_url)
    }

    #[cfg(test)]
    pub fn protocol_name(&self) -> Option<&'static str> {
        self.http().and_then(|http| http.protocol)
    }

    #[cfg(test)]
    pub fn max_tokens(&self) -> Option<u32> {
        self.http().and_then(|http| http.max_tokens)
    }

    #[cfg(test)]
    pub fn temperature(&self) -> Option<f32> {
        self.http().and_then(|http| http.temperature)
    }

    /// This preset's starting point for the per-model parameters.
    ///
    /// What `drep init` wrote before the quirks registry existed, and what
    /// every path that cannot consult it still writes: the `--provider` flag
    /// path, an offline run, and any model the registry does not name.
    pub fn quirks(&self) -> crate::llm::quirks::Quirks {
        match self.http() {
            Some(http) => crate::llm::quirks::Quirks {
                temperature: http.temperature,
                max_tokens: http.max_tokens,
                max_tokens_from_registry: false,
            },
            None => crate::llm::quirks::Quirks {
                temperature: None,
                max_tokens: None,
                max_tokens_from_registry: false,
            },
        }
    }

    /// The wire protocol this preset's endpoint speaks.
    ///
    /// The table stores a string because that is what `drep.toml` carries and
    /// what `config_file::render_one` writes. Parsing it here rather than at
    /// each use means a typo in the table is a panic in the preset tests rather
    /// than an `unwrap_or_default()` that silently builds an OpenAI client for
    /// an Anthropic endpoint.
    pub fn protocol(&self) -> open_agent::ApiProtocol {
        let http = self
            .http()
            .unwrap_or_else(|| panic!("preset `{}` has no HTTP wire protocol", self.key));
        crate::config::parse_protocol(http.protocol)
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
