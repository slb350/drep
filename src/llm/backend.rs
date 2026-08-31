//! Backend-specific request machinery behind the provider-chain contract.

use crate::config::{BackendKind, LlmConfig};
use crate::llm::client::LlmClient;
use crate::llm::codex::{CodexClient, CodexRuntime, CodexSettings};
use crate::llm::error::LlmError;
use crate::llm::json_parsing::Extracted;

/// One provider's concrete execution backend.
#[derive(Debug)]
pub enum ProviderBackend {
    Http(LlmClient),
    Codex(CodexClient),
}

impl ProviderBackend {
    pub fn model(&self) -> &str {
        match self {
            Self::Http(client) => client.model(),
            Self::Codex(client) => client.model(),
        }
    }

    /// Stable, non-personal identity used by the response cache.
    pub fn identity(&self) -> String {
        match self {
            Self::Http(client) => format!("http:{}", client.endpoint()),
            Self::Codex(client) => client.identity(),
        }
    }

    /// Backend-neutral location shown in reports.
    pub fn location(&self) -> &str {
        match self {
            Self::Http(client) => client.endpoint(),
            Self::Codex(_) => "codex://chatgpt",
        }
    }

    /// Request discriminator independent of provider identity and model.
    pub fn request_identity(&self) -> &str {
        match self {
            Self::Http(client) => client.request_identity(),
            Self::Codex(_) => "codex-jsonl-v1",
        }
    }

    pub fn temperature(&self) -> Option<f32> {
        match self {
            Self::Http(client) => client.temperature(),
            Self::Codex(_) => None,
        }
    }

    pub async fn complete_json(
        &self,
        system_prompt: &str,
        user_content: &str,
    ) -> Result<Extracted, LlmError> {
        match self {
            Self::Http(client) => client.complete_json(system_prompt, user_content).await,
            Self::Codex(client) => client.complete_json(system_prompt, user_content).await,
        }
    }

    #[cfg(test)]
    pub(crate) fn http_mut(&mut self) -> Option<&mut LlmClient> {
        match self {
            Self::Http(client) => Some(client),
            Self::Codex(_) => None,
        }
    }
}

/// Builds all backends in a chain while sharing process-wide backend state.
pub(crate) struct BackendFactory {
    codex_runtime: Option<Result<CodexRuntime, LlmError>>,
}

impl BackendFactory {
    pub(crate) fn new() -> Self {
        Self {
            codex_runtime: None,
        }
    }

    pub(crate) fn build(&mut self, cfg: &LlmConfig) -> Result<ProviderBackend, LlmError> {
        self.build_with(cfg, CodexRuntime::current)
    }

    fn build_with(
        &mut self,
        cfg: &LlmConfig,
        load_codex: impl FnOnce() -> Result<CodexRuntime, LlmError>,
    ) -> Result<ProviderBackend, LlmError> {
        match cfg.backend {
            BackendKind::Http => LlmClient::new(cfg).map(ProviderBackend::Http),
            BackendKind::Codex => {
                let settings = CodexSettings::from_config(cfg)?;
                match self.codex_runtime.get_or_insert_with(load_codex) {
                    Ok(runtime) => Ok(ProviderBackend::Codex(runtime.client(settings))),
                    Err(err) => Err(err.clone()),
                }
            }
            BackendKind::Unknown(ref name) => Err(LlmError::NotConfigured(format!(
                "unknown LLM backend `{name}`"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_failed_codex_probe_is_reused_for_the_whole_chain() {
        let cfg = LlmConfig {
            backend: BackendKind::Codex,
            model: Some("gpt-test".to_owned()),
            ..LlmConfig::default()
        };
        let calls = Cell::new(0);
        let mut factory = BackendFactory::new();

        for _ in 0..2 {
            let err = factory
                .build_with(&cfg, || {
                    calls.set(calls.get() + 1);
                    Err(LlmError::NotConfigured("not logged in".to_owned()))
                })
                .expect_err("diagnostic fails");
            assert!(err.to_string().contains("not logged in"));
        }

        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn invalid_codex_config_is_rejected_before_the_runtime_probe() {
        let cfg = LlmConfig {
            backend: BackendKind::Codex,
            model: None,
            ..LlmConfig::default()
        };
        let calls = Cell::new(0);
        let mut factory = BackendFactory::new();

        let err = factory
            .build_with(&cfg, || {
                calls.set(calls.get() + 1);
                Err(LlmError::NotConfigured("probe should not run".to_owned()))
            })
            .expect_err("the missing model is invalid locally");

        assert!(err.to_string().contains("model"), "got {err}");
        assert_eq!(calls.get(), 0);
    }
}
