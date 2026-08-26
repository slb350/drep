//! Resolving a provider credential by running a configured argv.
//!
//! Two halves are pinned here: the executor's own failure vocabulary, and
//! `resolve`'s precedence - an explicit `api_key` still wins, and a parked entry
//! spends nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::*;
use crate::auth::command::{self, KeyCommandError};
use crate::config::LlmConfig;
use crate::test_support::write_executable;

/// A stub program in `dir` that prints `stdout`, writes nothing else, exits 0.
fn stub_printing(dir: &Path, name: &str, stdout: &str) -> PathBuf {
    let path = dir.join(name);
    write_executable(&path, format!("#!/bin/sh\nprintf '%s' '{stdout}'\n"));
    path
}

/// A single-entry config whose credential comes from `argv`.
fn config_running(argv: &[&PathBuf]) -> Config {
    Config {
        max_review_rounds: crate::config::DEFAULT_MAX_REVIEW_ROUNDS,
        llm: vec![LlmConfig {
            endpoint: Some("https://gateway.example/v1".to_owned()),
            model: Some("m".to_owned()),
            api_key_command: Some(
                argv.iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
            ),
            ..LlmConfig::default()
        }],
    }
}

/// The executor's own timeout, long enough that a stub never trips it.
fn generous() -> Duration {
    Duration::from_secs(command::TIMEOUT_SECS)
}

#[tokio::test]
async fn a_command_supplies_the_key_when_the_config_names_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = stub_printing(dir.path(), "print-token", "minted-token");
    let mut config = config_running(&[&stub]);

    let sources = resolve(&mut config, &AuthStore::new())
        .await
        .expect("the command succeeds");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("minted-token"));
    assert_eq!(sources, vec![KeySource::Command]);
}

#[tokio::test]
async fn an_explicit_api_key_wins_and_the_command_never_runs() {
    // The documented precedence CI depends on. A stub that leaves a file behind
    // is what makes "did not run" checkable - asserting only on the resolved
    // value would also pass if the command ran and lost the race.
    let dir = tempfile::tempdir().expect("tempdir");
    let sentinel = dir.path().join("ran");
    let stub = dir.path().join("print-token");
    write_executable(
        &stub,
        format!(
            "#!/bin/sh\n: > '{}'\nprintf '%s' 'from-command'\n",
            sentinel.display()
        ),
    );
    let mut config = config_running(&[&stub]);
    config.llm[0].api_key = Some("from-config".to_owned());

    let sources = resolve(&mut config, &AuthStore::new())
        .await
        .expect("nothing to fail");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("from-config"));
    assert_eq!(sources, vec![KeySource::Config]);
    assert!(
        !sentinel.exists(),
        "the command must not run when the file already says where the key comes from"
    );
}

#[tokio::test]
async fn a_command_wins_over_a_stored_key_for_the_same_endpoint() {
    // The store fills in what the file left unset. A file naming a command has
    // not left it unset, so consulting the store first would make the file lie
    // about what the run used.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = stub_printing(dir.path(), "print-token", "from-command");
    let mut config = config_running(&[&stub]);
    let mut store = AuthStore::new();
    store
        .set("https://gateway.example/v1", "from-store")
        .expect("set");

    let sources = resolve(&mut config, &store)
        .await
        .expect("the stub succeeds");

    assert_eq!(config.llm[0].api_key.as_deref(), Some("from-command"));
    assert_eq!(sources, vec![KeySource::Command]);
}

#[tokio::test]
async fn a_disabled_entry_never_runs_its_command() {
    // A parked provider is inert in every other pass. Spending a real
    // credential call for one is worse than the missing-key report it avoids:
    // some helpers are rate-limited, and some prompt for a fingerprint.
    let dir = tempfile::tempdir().expect("tempdir");
    let sentinel = dir.path().join("ran");
    let stub = dir.path().join("print-token");
    write_executable(
        &stub,
        format!(
            "#!/bin/sh\n: > '{}'\nprintf '%s' 'tok'\n",
            sentinel.display()
        ),
    );
    let mut config = config_running(&[&stub]);
    config.llm[0].enabled = false;

    let sources = resolve(&mut config, &AuthStore::new())
        .await
        .expect("a parked entry cannot fail");

    assert_eq!(config.llm[0].api_key, None);
    assert_eq!(sources, vec![KeySource::Missing]);
    assert!(!sentinel.exists(), "a parked entry spends nothing");
}

#[tokio::test]
async fn a_failing_command_is_fatal_and_names_the_entry_in_file_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("print-token");
    write_executable(&stub, "#!/bin/sh\nexit 3\n");
    let mut config = config_running(&[&stub]);
    config.llm.insert(
        0,
        LlmConfig {
            endpoint: Some("http://localhost:1234/v1".to_owned()),
            model: Some("first".to_owned()),
            ..LlmConfig::default()
        },
    );

    let err = resolve(&mut config, &AuthStore::new())
        .await
        .expect_err("a broken credential path is fatal, not a provider failure");

    let message = err.to_string();
    assert!(message.contains("#2 in file order"), "got {message}");
    assert!(message.contains("api_key_command"), "got {message}");
}

#[tokio::test]
async fn a_trailing_newline_is_not_part_of_the_credential() {
    // Every helper that prints a token prints a newline after it, and a header
    // value carrying one is a guaranteed transport failure.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("print-token");
    write_executable(&stub, "#!/bin/sh\necho tok\n");

    let key = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect("a newline-terminated token is the normal case");

    assert_eq!(key, "tok");
}

#[tokio::test]
async fn a_command_that_prints_nothing_is_an_error_naming_the_program() {
    // An empty credential satisfies every "is a key present" check and then
    // 401s, which reads as a rejected key rather than a helper that printed
    // nothing. `AuthStore::set` refuses an empty paste for the same reason.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = stub_printing(dir.path(), "silent", "");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("an empty credential is not a credential");

    assert!(matches!(err, KeyCommandError::Empty { .. }), "got {err:?}");
    assert!(err.to_string().contains("silent"), "got {err}");
}

#[tokio::test]
async fn a_failing_command_reports_the_program_and_status_but_never_its_output() {
    // The security-critical case. A misconfigured helper can print the token to
    // either stream - `vault read -field=token` writes usage to stderr and the
    // value to stdout, and a wrapper that swaps them is one typo away - so the
    // diagnostic names the program and the status and nothing else.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("noisy");
    write_executable(
        &stub,
        "#!/bin/sh\nprintf '%s' 'sk-live-sekrit'\nprintf '%s' 'sk-live-sekrit' >&2\nexit 7\n",
    );

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("a nonzero exit is a failure");

    let message = err.to_string();
    assert!(message.contains("noisy"), "got {message}");
    assert!(message.contains('7'), "got {message}");
    assert!(
        !message.contains("sk-live-sekrit"),
        "the credential reached the diagnostic: {message}"
    );
    assert!(
        !format!("{err:?}").contains("sk-live-sekrit"),
        "the credential reached Debug: {err:?}"
    );
}

#[tokio::test]
async fn a_program_that_does_not_exist_is_reported_as_not_found() {
    // "could not be started: No such file or directory" sends the reader
    // looking at permissions. The actionable half is that argv[0] is not on
    // PATH, which is what a typo in the config produces.
    let err = command::run(
        &["drep-no-such-credential-helper-xyz".to_owned()],
        generous(),
    )
    .await
    .expect_err("a missing program cannot mint a key");

    assert!(
        matches!(err, KeyCommandError::NotFound { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("PATH"), "got {err}");
}

#[tokio::test]
async fn an_empty_argv_is_reported_rather_than_panicking() {
    // `config::load` rejects `api_key_command = []` for an enabled entry, so
    // this is unreachable from a file. It is still checked, because a panic
    // inside the commit gate is a worse failure than a message.
    let err = command::run(&[], generous())
        .await
        .expect_err("there is no program to run");

    assert!(matches!(err, KeyCommandError::NoProgram), "got {err:?}");
}

#[tokio::test]
async fn a_hung_command_is_bounded_by_the_supplied_timeout() {
    // The timeout is a parameter rather than read from the constant so this can
    // bound a wedged helper in milliseconds. `timeout_secs` is deliberately not
    // reused for it: that field is a model-response budget the presets set to
    // 1800, and a commit gate must not wait half an hour to learn a credential
    // helper is stuck.
    let err = command::run(
        &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        Duration::from_millis(100),
    )
    .await
    .expect_err("an unbounded child would hang the gate");

    assert!(
        matches!(err, KeyCommandError::Timeout { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn an_interior_newline_makes_the_credential_unusable() {
    // A two-line helper output cannot be sent as a header value at all, so it
    // is refused here rather than deferred to the first request - the same rule
    // `AuthStore::set` applies to a pasted key.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("two-lines");
    write_executable(&stub, "#!/bin/sh\nprintf 'a\\nb\\n'\n");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("a header value cannot carry a newline");

    assert!(
        matches!(err, KeyCommandError::Unusable { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn stdout_is_taken_whole_rather_than_scanned_for_a_token_shaped_line() {
    // No line or pattern scan: a helper whose output happens to contain a
    // colon, a space or a `token=` prefix is passing all of that deliberately,
    // and picking a substring out of it would send a different credential than
    // the one the helper produced.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = stub_printing(dir.path(), "structured", "Bearer abc.def-ghi");

    let key = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect("the whole trimmed output is the credential");

    assert_eq!(key, "Bearer abc.def-ghi");
}

#[tokio::test]
async fn surrounding_whitespace_is_trimmed_at_both_ends() {
    // A leading space is one `printf ' %s\n'` or one `cut` field away, and it
    // cannot survive the wire: an HTTP header value has its leading whitespace
    // stripped in transit, so what is sent is not what was checked and the
    // endpoint answers 401 - which reads as a revoked key rather than a helper
    // printing an extra byte. A space also passes the control-character guard
    // below, so trimming one end alone accepted it silently. `AuthStore::set`
    // trims a pasted key at both ends, and two credential paths disagreeing
    // about the same value is the drift worth pinning. The interior of the value
    // is untouched: see the test above.
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("padded");
    write_executable(&stub, "#!/bin/sh\nprintf ' %s\\n' tok\n");

    let key = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect("padding is the helper's, not the credential's");

    assert_eq!(key, "tok");
}

/// A helper that exists but cannot be executed is not a helper that is absent.
///
/// `NotFound`'s own message sends the reader to PATH and to a typo in argv[0],
/// which is the wrong place to look at a file whose mode is wrong. Collapsing the
/// two arms compiles and passes every other test in this file.
#[cfg(unix)]
#[tokio::test]
async fn a_helper_that_cannot_be_executed_is_reported_separately_from_a_missing_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("not-executable");
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s' tok\n").expect("a file with no execute bit");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("a file that cannot be executed cannot mint a key");

    assert!(matches!(err, KeyCommandError::Spawn { .. }), "got {err:?}");
    let message = err.to_string();
    assert!(message.contains("could not be started"), "got {message}");
    assert!(
        !message.contains("PATH"),
        "the program was found; naming PATH sends the reader to the wrong fix: {message}"
    );
}

/// A helper killed by a signal has no exit code to report.
///
/// Reaching for `Failed` here would have to invent one, and 0 is the one value
/// that would read as success.
#[cfg(unix)]
#[tokio::test]
async fn a_helper_killed_by_a_signal_says_so_rather_than_inventing_a_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("suicidal");
    write_executable(&stub, "#!/bin/sh\nkill -TERM $$\n");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("a helper that did not finish did not mint a key");

    assert!(matches!(err, KeyCommandError::Signal { .. }), "got {err:?}");
    assert!(err.to_string().contains("signal"), "got {err}");
}

/// Output that is not text is refused rather than repaired.
///
/// `from_utf8_lossy` would substitute U+FFFD, which is not a control character
/// and so passes the header guard below it - the credential then differs from
/// what the helper printed and fails at the transport, which is the confusing
/// place this whole error vocabulary exists to avoid.
#[tokio::test]
async fn output_that_is_not_utf8_is_refused_rather_than_replaced_with_placeholders() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("binary");
    write_executable(&stub, "#!/bin/sh\nprintf '\\377\\376'\n");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("a credential drep cannot read is not a credential");

    assert!(
        matches!(err, KeyCommandError::NotUtf8 { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("binary"), "got {err}");
}

/// A helper's own exit is decisive even if it launched an unrelated descendant
/// that inherited stdout.
///
/// Reading the pipe to EOF before waiting makes that descendant hold credential
/// resolution open until the timeout, after the helper already printed a valid
/// token and exited successfully.
#[cfg(unix)]
#[tokio::test]
async fn a_grandchild_inheriting_stdout_cannot_hold_credential_resolution_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("background-child");
    write_executable(&stub, "#!/bin/sh\n(sleep 5) &\nprintf '%s' token\n");

    let key = command::run(
        &[stub.to_string_lossy().into_owned()],
        std::time::Duration::from_secs(2),
    )
    .await
    .expect("the direct helper exited successfully");

    assert_eq!(key, "token");
}

/// A helper that prints more than any credential can be is refused, not read.
///
/// The ceiling is what stops `api_key_command` pointed at the wrong program -
/// `cat` on a large file is one keystroke from `cat` on a token file - allocating
/// whatever it printed inside the commit gate. Refused rather than truncated: a
/// prefix of something that was never a credential is a 401 per file.
#[tokio::test]
async fn output_past_the_ceiling_is_refused_rather_than_read_whole() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("flood");
    write_executable(&stub, "#!/bin/sh\nhead -c 70000 /dev/zero | tr '\\0' a\n");

    let err = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect_err("70000 bytes is not a credential");

    assert!(
        matches!(err, KeyCommandError::TooMuchOutput { limit, .. } if limit == 64 * 1024),
        "got {err:?}"
    );
    // The diagnostic names the ceiling and never a byte of what was printed, the
    // rule every variant here follows.
    let message = err.to_string();
    assert!(message.contains("65536"), "names the ceiling: {message}");
    assert!(!message.contains("aaaa"), "and never the output: {message}");
}

/// The discriminating half: exactly at the ceiling is still a credential.
///
/// Without it, an off-by-one that refuses at the limit rather than past it passes
/// the test above. A 64 KiB credential is absurd, which is the point - the ceiling
/// is a bound on a misconfiguration, not a judgement about a plausible token.
#[tokio::test]
async fn output_exactly_at_the_ceiling_is_still_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = dir.path().join("brim");
    write_executable(&stub, "#!/bin/sh\nhead -c 65536 /dev/zero | tr '\\0' a\n");

    let key = command::run(&[stub.to_string_lossy().into_owned()], generous())
        .await
        .expect("exactly at the ceiling is within it");

    assert_eq!(key.len(), 64 * 1024);
}
