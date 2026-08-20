//! ChatGPT-subscription reviews through the separately installed Codex CLI.

mod capture;
mod command;
mod diagnostics;
mod events;
mod process;

use std::path::PathBuf;
use std::time::Duration;

use crate::config::{BackendKind, LlmConfig, ReasoningEffort};
use crate::llm::error::{BackendErrorKind, LlmError};
use crate::llm::json_parsing::Extracted;

use command::ChildEnvironment;

/// Redacted readiness facts safe for diagnostics and cache identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexStatus {
    cli_version: String,
}

impl CodexStatus {
    pub(crate) fn new(cli_version: impl Into<String>) -> Self {
        Self {
            cli_version: cli_version.into(),
        }
    }

    pub(crate) fn cli_version(&self) -> &str {
        &self.cli_version
    }
}

/// Verify that the installed Codex CLI is using ChatGPT-managed credentials.
///
/// The underlying diagnostic may include account paths and identifiers. Only
/// the CLI version and the successful authentication classification cross this
/// boundary.
pub(crate) fn current_status() -> Result<CodexStatus, String> {
    CodexRuntime::probe_current()
        .map(|runtime| CodexStatus::new(runtime.cli_version))
        .map_err(|err| err.to_string())
}

/// Process state shared by every Codex provider in one configured chain.
///
/// Authentication is account-wide, not model-specific. Probing once avoids
/// launching the CLI repeatedly when the chain names more than one Codex
/// model, while keeping the redacted result local to this run.
#[derive(Debug, Clone)]
pub(crate) struct CodexRuntime {
    executable: PathBuf,
    environment: ChildEnvironment,
    cli_version: String,
}

impl CodexRuntime {
    pub(crate) fn current() -> Result<Self, LlmError> {
        Self::probe_current().map_err(|err| LlmError::NotConfigured(err.to_string()))
    }

    fn probe_current() -> Result<Self, diagnostics::DiagnosticError> {
        let executable = PathBuf::from("codex");
        let environment = ChildEnvironment::current();
        let status = diagnostics::probe(&executable, &environment)?;
        Ok(Self {
            executable,
            environment,
            cli_version: status.cli_version,
        })
    }

    pub(crate) fn client(&self, settings: CodexSettings) -> CodexClient {
        CodexClient::from_settings(
            settings,
            self.executable.clone(),
            self.environment.clone(),
            self.cli_version.clone(),
        )
    }
}

/// Provider fields validated before any Codex process is started.
pub(crate) struct CodexSettings {
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
    timeout_secs: u64,
}

impl CodexSettings {
    pub(crate) fn from_config(cfg: &LlmConfig) -> Result<Self, LlmError> {
        if !cfg.enabled {
            return Err(LlmError::NotConfigured(
                "LLM is disabled in config (set `enabled = true`)".to_owned(),
            ));
        }
        if cfg.backend != BackendKind::Codex {
            return Err(LlmError::NotConfigured(
                "Codex client requires `backend = \"codex\"`".to_owned(),
            ));
        }
        let model = cfg
            .model
            .clone()
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| LlmError::NotConfigured("LLM model is not set in config".to_owned()))?;
        let reasoning_effort = cfg.reasoning_effort.clone();
        if matches!(reasoning_effort, Some(ReasoningEffort::Unknown(_))) {
            return Err(LlmError::NotConfigured(
                "Codex reasoning_effort is not recognised".to_owned(),
            ));
        }
        Ok(Self {
            model,
            reasoning_effort,
            timeout_secs: cfg.timeout_secs,
        })
    }
}

/// A configured ChatGPT-subscription client.
pub struct CodexClient {
    executable: PathBuf,
    model: String,
    reasoning_effort: Option<ReasoningEffort>,
    timeout_secs: u64,
    cli_version: String,
    environment: ChildEnvironment,
}

impl std::fmt::Debug for CodexClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexClient")
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("timeout_secs", &self.timeout_secs)
            .field("cli_version", &self.cli_version)
            .finish()
    }
}

impl CodexClient {
    /// Inject process state for tests without changing PATH or the environment.
    #[cfg(test)]
    pub(crate) fn at(
        cfg: &LlmConfig,
        executable: PathBuf,
        environment: ChildEnvironment,
        cli_version: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let settings = CodexSettings::from_config(cfg)?;
        let cli_version = cli_version.into();
        if cli_version.is_empty() {
            return Err(LlmError::NotConfigured(
                "Codex CLI diagnostic did not report a version".to_owned(),
            ));
        }
        Ok(Self::from_settings(
            settings,
            executable,
            environment,
            cli_version,
        ))
    }

    fn from_settings(
        settings: CodexSettings,
        executable: PathBuf,
        environment: ChildEnvironment,
        cli_version: String,
    ) -> Self {
        Self {
            executable,
            model: settings.model,
            reasoning_effort: settings.reasoning_effort,
            timeout_secs: settings.timeout_secs,
            cli_version,
            environment,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        cfg: &LlmConfig,
        executable: impl Into<PathBuf>,
        environment: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
        cli_version: impl Into<String>,
    ) -> Result<Self, LlmError> {
        Self::at(
            cfg,
            executable.into(),
            ChildEnvironment::from_iter(environment),
            cli_version,
        )
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    #[cfg(test)]
    pub(crate) fn cli_version(&self) -> &str {
        &self.cli_version
    }

    #[cfg(test)]
    pub(crate) fn reasoning_effort(&self) -> Option<&ReasoningEffort> {
        self.reasoning_effort.as_ref()
    }

    /// Stable, non-personal identity for cache and reporting.
    pub fn identity(&self) -> String {
        format!(
            "codex:chatgpt:cli={}:effort={}",
            self.cli_version,
            self.reasoning_effort
                .as_ref()
                .map_or("default", ReasoningEffort::as_str)
        )
    }

    pub async fn complete_json(
        &self,
        system_prompt: &str,
        user_content: &str,
    ) -> Result<Extracted, LlmError> {
        let workspace = tempfile::Builder::new()
            .prefix("drep-codex-")
            .tempdir()
            .map_err(|err| {
                LlmError::NotConfigured(format!("could not create Codex workspace: {err}"))
            })?;
        let instructions = workspace.path().join("instructions.md");
        let schema = workspace.path().join("schema.json");
        let cwd = workspace.path().join("cwd");
        std::fs::create_dir(&cwd).map_err(|err| {
            LlmError::NotConfigured(format!("could not create empty Codex cwd: {err}"))
        })?;
        std::fs::write(&instructions, command::instructions_text(system_prompt)).map_err(
            |err| LlmError::NotConfigured(format!("could not write Codex instructions: {err}")),
        )?;
        std::fs::write(
            &schema,
            crate::analysis::response_contract::output_schema_bytes(),
        )
        .map_err(|err| {
            LlmError::NotConfigured(format!("could not write Codex response schema: {err}"))
        })?;

        let args = command::invocation_args(
            &self.model,
            self.reasoning_effort.as_ref(),
            &instructions,
            &schema,
            &cwd,
        )
        .map_err(|err| LlmError::NotConfigured(err.to_string()))?;
        let output = process::run(
            &self.executable,
            &args,
            &self.environment,
            &cwd,
            user_content,
            Duration::from_secs(self.timeout_secs),
        )
        .await?;
        if output.status.code().is_none() {
            return Err(LlmError::Transport {
                status: None,
                message: format!(
                    "Codex CLI terminated without an exit status: {}",
                    output.stderr_excerpt()
                ),
            });
        }
        if !output.status.success() {
            let detail = match events::parse_jsonl(output.stdout.as_slice()) {
                Err(events::EventError::ReportedError(message)) => message,
                _ => output.stderr_excerpt(),
            };
            return Err(LlmError::Backend {
                kind: BackendErrorKind::UnknownExit,
                message: format!(
                    "Codex CLI exited with status {}: {}",
                    output.status.code().expect("checked above"),
                    detail
                ),
            });
        }
        let value = events::parse_jsonl(output.stdout.as_slice()).map_err(map_event_error)?;
        Ok(Extracted::Complete(value))
    }
}

fn map_event_error(err: events::EventError) -> LlmError {
    match err {
        events::EventError::ReportedError(message) => LlmError::Backend {
            kind: BackendErrorKind::UnknownExit,
            message,
        },
        events::EventError::MalformedFinal(message) => LlmError::Unparseable(message),
        events::EventError::Read(_)
        | events::EventError::MissingFinalMessage
        | events::EventError::MissingTurnCompletion => LlmError::Transport {
            status: None,
            message: err.to_string(),
        },
        other => LlmError::Backend {
            kind: BackendErrorKind::Contract,
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests;
