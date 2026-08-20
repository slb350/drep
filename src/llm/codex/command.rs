//! Construction of the locked-down `codex exec` command.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::config::ReasoningEffort;
use thiserror::Error;

/// Environment variables the child is allowed to inherit.
#[derive(Clone, Debug, Default)]
pub(crate) struct ChildEnvironment {
    values: BTreeMap<OsString, OsString>,
}

impl ChildEnvironment {
    /// Filter a captured environment without reading or mutating process state.
    pub(crate) fn from_iter(values: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let values = values
            .into_iter()
            .filter(|(name, _)| allowed_name(name))
            .collect();
        Self { values }
    }

    /// Capture the production process environment once.
    pub(crate) fn current() -> Self {
        Self::from_iter(std::env::vars_os())
    }

    /// Apply the allowlist to a child after clearing its inherited environment.
    pub(crate) fn apply_to(&self, command: &mut tokio::process::Command) {
        self.apply_to_std(command.as_std_mut());
    }

    pub(crate) fn apply_to_std(&self, command: &mut std::process::Command) {
        command.env_clear().envs(self.values.iter());
    }

    #[cfg(test)]
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(OsStr::new(name))
            .and_then(|value| value.to_str())
    }
}

/// Build `key=<TOML string>` with the TOML serializer, not hand quoting.
pub(crate) fn toml_override(key: &str, value: &str) -> String {
    format!("{key}={}", toml::Value::String(value.to_owned()))
}

#[derive(Debug, Error)]
pub(crate) enum CommandError {
    #[error("Codex instructions path is not valid UTF-8")]
    NonUtf8InstructionsPath,
}

/// Exact ordered arguments for one isolated review.
pub(crate) fn invocation_args(
    model: &str,
    effort: Option<&ReasoningEffort>,
    instructions: &Path,
    schema: &Path,
    cwd: &Path,
) -> Result<Vec<OsString>, CommandError> {
    let instructions = instructions
        .to_str()
        .ok_or(CommandError::NonUtf8InstructionsPath)?;
    let schema = schema.as_os_str();
    let cwd = cwd.as_os_str();

    let mut args = vec![
        OsString::from("-c"),
        OsString::from(toml_override("forced_login_method", "chatgpt")),
        OsString::from("-c"),
        OsString::from(toml_override("model_instructions_file", instructions)),
    ];
    if let Some(effort) = effort {
        args.push(OsString::from("-c"));
        args.push(OsString::from(toml_override(
            "model_reasoning_effort",
            effort.as_str(),
        )));
    }
    args.extend([
        OsString::from("-c"),
        OsString::from("project_doc_max_bytes=0"),
        OsString::from("-c"),
        OsString::from(toml_override("web_search", "disabled")),
        OsString::from("--disable"),
        OsString::from("shell_tool"),
        OsString::from("--disable"),
        OsString::from("unified_exec"),
        OsString::from("--disable"),
        OsString::from("apps"),
        OsString::from("--disable"),
        OsString::from("multi_agent"),
        OsString::from("--disable"),
        OsString::from("hooks"),
        OsString::from("--disable"),
        OsString::from("memories"),
        OsString::from("-a"),
        OsString::from("never"),
        OsString::from("exec"),
        OsString::from("--ephemeral"),
        OsString::from("--ignore-user-config"),
        OsString::from("--ignore-rules"),
        OsString::from("--sandbox"),
        OsString::from("read-only"),
        OsString::from("--skip-git-repo-check"),
        OsString::from("-C"),
        cwd.to_owned(),
        OsString::from("--model"),
        OsString::from(model),
        OsString::from("--output-schema"),
        schema.to_owned(),
        OsString::from("--json"),
        OsString::from("-"),
    ]);
    Ok(args)
}

/// Replace Codex's generic agent instructions with drep's review contract.
pub(crate) fn instructions_text(system_prompt: &str) -> String {
    format!(
        "{system_prompt}\n\nIsolation contract:\n\
         - Use only the code supplied on stdin.\n\
         - Do not use tools, commands, files, network access, apps, MCP, or subagents.\n\
         - Return only the JSON object required by the response schema.\n"
    )
}

fn allowed_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if sensitive_name(name) {
        return false;
    }
    matches!(
        name,
        "PATH"
            | "HOME"
            | "CODEX_HOME"
            | "TMPDIR"
            | "TMP"
            | "TEMP"
            | "SSL_CERT_FILE"
            | "SSL_CERT_DIR"
            | "REQUESTS_CA_BUNDLE"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "ALL_PROXY"
            | "NO_PROXY"
            | "http_proxy"
            | "https_proxy"
            | "all_proxy"
            | "no_proxy"
            | "LANG"
            | "LC_ALL"
            | "TZ"
            | "SYSTEMROOT"
            | "WINDIR"
            | "USERPROFILE"
            | "APPDATA"
            | "LOCALAPPDATA"
            | "ComSpec"
            | "PATHEXT"
    ) || name.starts_with("LC_")
}

/// Secret-bearing environment-name patterns that stay excluded even if a
/// future allowlist entry accidentally names one.
pub(crate) fn sensitive_name(name: &str) -> bool {
    name.starts_with("DREP_") || name.ends_with("_API_KEY")
}
