//! Typed backend fields for one `[[llm]]` table.
//!
//! The parent module validates the raw TOML tree before forgetting whether a
//! defaulted field was explicit. This module owns the typed values themselves.

use std::collections::BTreeMap;

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
///
/// `deny_unknown_fields` because the alternative is what a misspelled key was
/// doing until this attribute arrived: serde dropped it without a word, and a
/// user who wrote `[llm.headers]` before drep could send one got a config that
/// read as configured and sent nothing. That is the same silent-drop failure
/// `ConfigError::SiteOnlyField` exists to refuse one file over, and there is no
/// reason for `drep.toml` to be laxer than the policy file about it.
///
/// It is the one pass that does not honour "a disabled entry is inert", because
/// serde rejects at deserialization and there is no entry yet to skip. So a
/// parked provider carrying a field from a newer drep refuses to load the file
/// rather than being ignored. That is the wanted trade - a typo in a parked
/// entry is still a typo, and the entry is one line from being re-enabled - but
/// it is a deviation from a rule `${VAR}` expansion, field validation and
/// credential resolution all keep.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    pub enabled: bool,
    pub backend: BackendKind,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// An argv - never a shell line - whose trimmed stdout is the credential.
    ///
    /// Declared after `api_key` because the field order is the resolution order:
    /// an explicit key wins, then this, then the per-machine store.
    pub api_key_command: Option<Vec<String>>,
    /// Extra HTTP headers sent with every request to this provider.
    ///
    /// For the gateway that identifies its clients by `User-Agent`, bills
    /// against a header, or authenticates outside its protocol's default
    /// scheme. A name that collides with one the protocol sets replaces it, so
    /// this can carry an `Authorization` the SDK's own scheme would not produce.
    ///
    /// **A value here can be a credential.** A project or tenant token is the
    /// ordinary case, which is the whole reason `Debug` here, `LlmClient`'s
    /// `Debug`, `doctor`'s listing and `ConfigError::UnusableHeaderValue` all
    /// print the name and never the value. This is the one place that argument
    /// is made; the others cite it.
    ///
    /// A `BTreeMap` rather than a list of pairs: a header set twice is one
    /// header, and the sorted order makes the rendered config and the `doctor`
    /// listing stable. `config::effective_headers` overlays it on drep's own
    /// defaults to get what is actually sent.
    pub headers: BTreeMap<String, String>,
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
            .field(
                "api_key_command",
                &self
                    .api_key_command
                    .as_deref()
                    .map(describe_command)
                    .unwrap_or_else(|| "None".to_owned()),
            )
            // Names only, spelled exactly as `LlmClient`'s `Debug` spells it:
            // a header value is as likely to be a credential as `api_key` is.
            .field(
                "headers",
                &self.headers.keys().map(String::as_str).collect::<Vec<_>>(),
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

/// The program name plus how many arguments follow it, never the arguments.
///
/// The program is the useful non-secret half, the same trade `AuthStore`'s
/// `Debug` makes by printing its endpoints. The arguments are not that half:
/// `["vault", "read", "--token=…"]` carries a credential in argv, and so does a
/// helper invoked as `["sh", "-c", "curl -H 'Authorization: …'"]`. Redacting
/// them individually would mean deciding which of them looks secret, which is
/// the judgement call this struct hand-writes `Debug` to avoid making.
fn describe_command(argv: &[String]) -> String {
    match argv.split_first() {
        None => "[]".to_owned(),
        Some((program, rest)) => format!("[{program}, {} argument(s) redacted]", rest.len()),
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
            api_key_command: None,
            headers: BTreeMap::new(),
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
    api_key_command: bool,
    headers: bool,
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
                    api_key_command: entry.get("api_key_command").is_some(),
                    headers: entry.get("headers").is_some(),
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
                (fields.api_key_command, "api_key_command"),
                (fields.headers, "headers"),
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

/// Fails to compile when a field is added to [`LlmConfig`] without deciding what
/// the three hand-maintained lists beside it should say.
///
/// `ExplicitFields`, `explicit_fields` and the Codex rejection list in
/// [`validate`] are parallel to this struct and kept in step by hand. Adding
/// `headers` took six coordinated edits and nothing would have failed had the
/// last two been missed - a Codex entry would simply have started accepting an
/// HTTP-only field, defeating the documented guarantee that a subscription
/// selection cannot silently become API billing. `deny_unknown_fields` cannot
/// catch that: the field is known, it is the lists that forgot it.
///
/// The same guard `config::site` uses for its policy fields, for the reason
/// stated there: a list kept in step with a type by hand is a list that drifts
/// silently.
#[cfg(test)]
fn _every_provider_field_is_classified(config: &LlmConfig) {
    let LlmConfig {
        // Not backend-specific: every entry has these whatever it runs.
        enabled: _,
        backend: _,
        model: _,
        max_concurrent: _,
        timeout_secs: _,
        // HTTP-only: each of these has a row in the Codex rejection list in
        // `validate`, and a field in `ExplicitFields` so the rejection can tell
        // "written" from "defaulted".
        endpoint: _,
        api_key: _,
        api_key_command: _,
        headers: _,
        protocol: _,
        temperature: _,
        max_tokens: _,
        max_retries: _,
        // Codex-only: rejected on an HTTP entry by the arm above.
        reasoning_effort: _,
    } = config;
}
