//! Typed backend fields for one `[[llm]]` table.
//!
//! The parent module validates the raw TOML tree before forgetting whether a
//! defaulted field was explicit. This module owns the typed values themselves.

use serde::{Deserialize, Deserializer};
use toml::Value;

use super::ConfigError;

/// Which implementation serves one provider entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BackendKind {
    /// Direct HTTP through `open-agent-sdk`.
    #[default]
    Http,
    /// The installed Codex CLI using its saved ChatGPT authentication.
    Codex,
    /// Retained only so a disabled entry remains inert; enabled entries reject it.
    Unknown(String),
}

impl BackendKind {
    /// Stable configuration and cache identity.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Http => "http",
            Self::Codex => "codex",
            Self::Unknown(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for BackendKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "http" => Self::Http,
            "codex" => Self::Codex,
            _ => Self::Unknown(value),
        })
    }
}

/// Reasoning effort accepted by the Codex CLI configuration contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    /// Retained only so a disabled entry remains inert; enabled entries reject it.
    Unknown(String),
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Unknown(value) => value,
        }
    }
}

impl<'de> Deserialize<'de> for ReasoningEffort {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" => Self::Xhigh,
            _ => Self::Unknown(value),
        })
    }
}

/// One provider in the failover chain.
#[derive(Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub backend: BackendKind,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub protocol: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub max_concurrent: usize,
}

/// Hand-written so the API key cannot reach a log.
impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("enabled", &self.enabled)
            .field("backend", &self.backend)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field(
                "api_key",
                &self
                    .api_key
                    .as_ref()
                    .map(|_| "<redacted>")
                    .unwrap_or("None"),
            )
            .field("protocol", &self.protocol)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("max_concurrent", &self.max_concurrent)
            .finish()
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: BackendKind::Http,
            endpoint: None,
            model: None,
            api_key: None,
            protocol: None,
            reasoning_effort: None,
            temperature: None,
            max_tokens: None,
            timeout_secs: 60,
            max_retries: 3,
            max_concurrent: 3,
        }
    }
}

/// Backend-sensitive fields explicitly present in one raw `[[llm]]` table.
///
/// Captured before TOML deserialization consumes the tree. Keeping booleans
/// avoids cloning the expanded tree, which may contain an API key.
#[derive(Clone, Copy, Default)]
pub(super) struct ExplicitFields {
    endpoint: bool,
    api_key: bool,
    protocol: bool,
    reasoning_effort: bool,
    temperature: bool,
    max_tokens: bool,
    max_retries: bool,
}

pub(super) fn explicit_fields(tree: &Value) -> Vec<ExplicitFields> {
    tree.get("llm")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| ExplicitFields {
                    endpoint: entry.get("endpoint").is_some(),
                    api_key: entry.get("api_key").is_some(),
                    protocol: entry.get("protocol").is_some(),
                    reasoning_effort: entry.get("reasoning_effort").is_some(),
                    temperature: entry.get("temperature").is_some(),
                    max_tokens: entry.get("max_tokens").is_some(),
                    max_retries: entry.get("max_retries").is_some(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn validate(
    llm: &LlmConfig,
    fields: ExplicitFields,
    index: usize,
) -> Result<(), ConfigError> {
    if let BackendKind::Unknown(value) = &llm.backend {
        return Err(ConfigError::UnknownBackend {
            index,
            value: value.clone(),
        });
    }
    if let Some(ReasoningEffort::Unknown(value)) = &llm.reasoning_effort {
        return Err(ConfigError::UnknownReasoningEffort {
            index,
            value: value.clone(),
        });
    }

    match &llm.backend {
        BackendKind::Http if fields.reasoning_effort => Err(ConfigError::BackendField {
            index,
            backend: "http",
            field: "reasoning_effort",
        }),
        BackendKind::Http => Ok(()),
        BackendKind::Codex
            if llm
                .model
                .as_deref()
                .is_none_or(|model| model.trim().is_empty()) =>
        {
            Err(ConfigError::BackendMissingField {
                index,
                backend: "codex",
                field: "model",
            })
        }
        BackendKind::Codex => {
            for (present, field) in [
                (fields.endpoint, "endpoint"),
                (fields.api_key, "api_key"),
                (fields.protocol, "protocol"),
                (fields.temperature, "temperature"),
                (fields.max_tokens, "max_tokens"),
                (fields.max_retries, "max_retries"),
            ] {
                if present {
                    return Err(ConfigError::BackendField {
                        index,
                        backend: "codex",
                        field,
                    });
                }
            }
            Ok(())
        }
        BackendKind::Unknown(_) => unreachable!("handled above"),
    }
}
