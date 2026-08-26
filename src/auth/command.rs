//! Resolving a provider credential by running a configured argv.
//!
//! For a gateway whose tokens expire in minutes, a stored key is stale before
//! the second commit. The recognisable shape is a helper the user already runs
//! by hand - `gcloud auth print-access-token`, `az account get-access-token`,
//! `op read`, `vault read` - so `api_key_command` is that argv, run with no
//! shell, and its trimmed stdout is the credential.
//!
//! Three decisions are load-bearing:
//!
//! - **The diagnostic is deliberately thin.** A failure reports the program and
//!   the exit status and nothing else. A misconfigured helper can print the
//!   token to either stream - `vault read -field=token` writes the value to
//!   stdout and its usage to stderr, and a wrapper that swaps them is one typo
//!   away - so an error message is the one place a credential would escape into
//!   a terminal, a CI log or a bug report.
//! - **One variant per distinguishable cause.** "could not be started",
//!   "exited 7", "printed nothing" and "printed something unusable" send the
//!   reader to four different fixes, and collapsing them into one message about
//!   the command failing sends them to none.
//! - **No disk cache and no TTL.** drep is a short-lived process, so a
//!   credential written to disk buys nothing and adds a file worth stealing.
//!   `auth::resolve` runs each entry's command exactly once per process instead.

use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;

/// How long a credential helper may take.
///
/// Not `timeout_secs`: that field is a model-response budget the presets set to
/// 1800, and a commit gate must not wait half an hour to learn a helper is
/// wedged. Thirty seconds covers a network round trip to a token endpoint and an
/// interactive approval, which is the slowest thing any of the recognisable
/// helpers does.
pub(crate) const TIMEOUT_SECS: u64 = 30;

/// Why a configured `api_key_command` did not produce a usable credential.
///
/// `Debug` is derived, which is safe only because no variant carries captured
/// output. Adding one that did would put a credential into `{:?}`.
#[derive(Debug, Error)]
pub enum KeyCommandError {
    /// Reachable only from a hand-built `LlmConfig`: `config::load` rejects
    /// `api_key_command = []` for an enabled entry. Checked anyway, because a
    /// panic inside the commit gate is a worse failure than a message.
    #[error("api_key_command is empty; it must name a program to run")]
    NoProgram,

    /// Distinguished from [`Self::Spawn`] because the fix is different: this one
    /// is a typo in argv[0] or a helper that is not installed, and reporting it
    /// as "could not be started: No such file or directory" sends the reader
    /// looking at file permissions instead.
    #[error("api_key_command `{program}` was not found on PATH")]
    NotFound { program: String },

    #[error("api_key_command `{program}` could not be started: {cause}")]
    Spawn {
        program: String,
        cause: std::io::Error,
    },

    #[error("api_key_command `{program}` did not finish within {secs} seconds")]
    Timeout { program: String, secs: u64 },

    /// A helper killed by a signal has no exit code, so it cannot be reported
    /// through [`Self::Failed`] without inventing one.
    #[error("api_key_command `{program}` was killed by a signal")]
    Signal { program: String },

    /// The status and nothing else. See the module doc: captured output is where
    /// a credential would escape.
    #[error("api_key_command `{program}` exited {code}")]
    Failed { program: String, code: i32 },

    #[error(
        "api_key_command `{program}` printed output that is not valid UTF-8; \
         it must print the credential as text on stdout"
    )]
    NotUtf8 { program: String },

    /// An empty credential satisfies every "is a key present" check downstream
    /// and then 401s, which reads as a rejected key rather than a helper that
    /// printed nothing. `AuthStore::set` refuses an empty paste for the same
    /// reason.
    #[error(
        "api_key_command `{program}` printed nothing; the whole trimmed stdout is \
         the credential, so it must print one"
    )]
    Empty { program: String },

    /// The resolved value becomes an HTTP header, which cannot carry a control
    /// character - so a helper that printed two lines, or a diagnostic banner
    /// followed by the token, is a guaranteed transport failure. Caught here
    /// rather than on the first file of the first push, exactly as
    /// `AuthStore::set` catches it at the prompt.
    #[error(
        "api_key_command `{program}` printed a value containing a character that \
         cannot be sent in a header; it must print the credential alone"
    )]
    Unusable { program: String },
}

/// [`run`] bounded by drep's own [`TIMEOUT_SECS`].
///
/// The one place the constant is applied. `check` and `doctor` both run a
/// helper, and two call sites each building their own `Duration` could come to
/// bound the same helper differently - which would have `doctor` report on a run
/// `check` abandons.
pub(crate) async fn run_bounded(argv: &[String]) -> Result<String, KeyCommandError> {
    run(argv, Duration::from_secs(TIMEOUT_SECS)).await
}

/// Run `argv` and return its trimmed stdout as the credential.
///
/// `timeout` is a parameter rather than read from [`TIMEOUT_SECS`] so a test can
/// bound a wedged helper in milliseconds, the same reason `LlmClient`'s
/// `retry_config` is reachable.
///
/// stdin is closed rather than inherited: a helper that decides to prompt would
/// otherwise read the terminal drep's caller is using, and a git hook has no
/// terminal to give it. Both output streams are piped and drained by
/// `wait_with_output`, so a chatty helper cannot fill a pipe and deadlock, and
/// `kill_on_drop` reaps the child on the timeout branch.
pub(crate) async fn run(argv: &[String], timeout: Duration) -> Result<String, KeyCommandError> {
    let Some((program, arguments)) = argv.split_first() else {
        return Err(KeyCommandError::NoProgram);
    };

    let mut command = tokio::process::Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = command.spawn().map_err(|cause| match cause.kind() {
        std::io::ErrorKind::NotFound => KeyCommandError::NotFound {
            program: program.clone(),
        },
        _ => KeyCommandError::Spawn {
            program: program.clone(),
            cause,
        },
    })?;

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(cause)) => {
            return Err(KeyCommandError::Spawn {
                program: program.clone(),
                cause,
            });
        }
        Err(_) => {
            return Err(KeyCommandError::Timeout {
                program: program.clone(),
                secs: timeout.as_secs(),
            });
        }
    };

    // The status is consulted before the output, so a helper that failed *and*
    // printed a partial value is reported as failed. `String::from_utf8` on the
    // stdout of a helper that exited 7 would otherwise decide which of the two
    // problems the user hears about.
    if !output.status.success() {
        return Err(match output.status.code() {
            Some(code) => KeyCommandError::Failed {
                program: program.clone(),
                code,
            },
            None => KeyCommandError::Signal {
                program: program.clone(),
            },
        });
    }

    // Taken whole, with only trailing whitespace removed: every helper that
    // prints a token prints a newline after it. Deliberately no scan for a
    // line, a prefix or a token-shaped pattern - a helper's output is what its
    // author chose to emit, and picking a substring out of it would send a
    // different credential than the one the helper produced.
    let key = String::from_utf8(output.stdout)
        .map_err(|_| KeyCommandError::NotUtf8 {
            program: program.clone(),
        })?
        .trim_end()
        .to_owned();

    if key.is_empty() {
        return Err(KeyCommandError::Empty {
            program: program.clone(),
        });
    }
    if key.chars().any(char::is_control) {
        return Err(KeyCommandError::Unusable {
            program: program.clone(),
        });
    }
    Ok(key)
}
