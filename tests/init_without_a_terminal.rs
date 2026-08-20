//! `drep init` must not prompt when there is nobody to answer.
//!
//! The decision itself is unit-tested through `wants_wizard`, which takes the
//! terminal check as a parameter. What no unit test can cover is the *wiring*:
//! `std::io::stdin().is_terminal()` is always false under `cargo test` - the
//! harness captures stdin - so an `is_interactive` that ignored it entirely and
//! returned `true` would pass the whole in-process suite while hanging or
//! failing every hook and CI job that ran the real binary.
//!
//! Running the binary with a piped stdin is the only thing that tells those two
//! apart, which is why this lives here rather than beside the other init tests.

use std::process::{Command, Stdio};

/// A fresh git repository to install into.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for args in [
        vec!["init", "--initial-branch=main"],
        vec!["config", "--local", "user.email", "test@example.com"],
        vec!["config", "--local", "user.name", "test"],
        // A globally-set value would otherwise leak in and send hook
        // installation at the developer's real shared hooks directory.
        vec!["config", "--local", "core.hooksPath", ""],
    ] {
        let status = Command::new("git")
            .args(&args)
            .current_dir(dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git must run");
        assert!(status.success(), "git {args:?} failed");
    }
    dir
}

/// Write a cache the quirks registry will accept as fresh, and return its path.
///
/// Empty of providers on purpose: what these tests exercise is the wiring
/// around the wizard, not the registry, and every model they configure is
/// therefore one it does not name - which is the documented fallback to the
/// preset's own values. Stamped with the current wall clock so it is inside
/// the one-week freshness window and no fetch is attempted.
fn seed_quirks_cache(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("model-quirks.toml");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    std::fs::write(&path, format!("fetched_at = {now}\n")).expect("seed the quirks cache");
    path
}

/// An endpoint nothing listens on.
///
/// Every wizard script here names one explicitly rather than accepting a
/// preset's default. Two reasons, both about determinism: the wizard now asks
/// the endpoint which models it serves, so a default pointing at a real vendor
/// would make these tests issue live requests to a third party, and one
/// pointing at `localhost:1234` would behave differently on a machine with LM
/// Studio running. Port 9 refuses immediately, which exercises the
/// could-not-list fallback offline and fast.
const DEAD: &str = "http://127.0.0.1:9/v1";

/// The `custom` preset's position in the list the wizard prints.
///
/// Computed rather than pinned: a hardcoded number fails by selecting a
/// *different* provider when a preset is added, which passes compilation and
/// then asserts against the wrong thing. `custom` is the preset that needs both
/// an endpoint and a key, which is what makes it the right vehicle for the
/// key-storing cases.
fn custom() -> String {
    let index = drep::cli::init::presets::PRESETS
        .iter()
        .position(|preset| preset.key == "custom")
        .expect("the custom preset exists");
    (index + 1).to_string()
}

/// Run `drep init` in `dir` with the given extra arguments and a closed stdin.
fn init_with_closed_stdin(dir: &tempfile::TempDir, extra: &[&str]) -> std::process::Output {
    run_init(dir, extra, None)
}

/// Run `drep init`, optionally writing `answers` to its stdin.
///
/// `None` closes stdin entirely; `Some` pipes the text, which is still not a
/// terminal - that is the point.
fn run_init(
    dir: &tempfile::TempDir,
    extra: &[&str],
    answers: Option<&str>,
) -> std::process::Output {
    use std::io::Write;

    let mut command = Command::new(env!("CARGO_BIN_EXE_drep"));
    command
        .arg("init")
        .args(extra)
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A store inside the temp dir, so the run cannot read or rewrite the
        // developer's real keys.
        .env("DREP_AUTH_PATH", dir.path().join("auth.toml"))
        // And a model-quirks cache inside it too, seeded fresh by
        // `seed_quirks_cache`. Without this the interactive runs below fetch
        // 4 MB from models.dev and write the result into the developer's real
        // config directory - beside `auth.toml`, and creating that directory
        // if it did not exist. Same reason as the line above, and the same
        // reason this file names a dead endpoint: a test issues no live
        // request to a third party.
        .env("DREP_QUIRKS_PATH", seed_quirks_cache(dir));

    match answers {
        None => {
            command.stdin(Stdio::null());
            command.output().expect("drep must run")
        }
        Some(text) => {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().expect("drep must run");
            child
                .stdin
                .as_mut()
                .expect("stdin is piped")
                .write_all(text.as_bytes())
                .expect("write answers");
            child.wait_with_output().expect("drep must finish")
        }
    }
}

#[test]
fn a_piped_stdin_takes_the_flag_path_rather_than_prompting() {
    let dir = repo();

    let output = init_with_closed_stdin(&dir, &["--hooks", "none", "--no-gitignore"]);

    assert!(
        output.status.success(),
        "init failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        dir.path().join("drep.toml").exists(),
        "the config was written, so the flag path ran"
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("Which provider?"),
        "the wizard ran with nobody to answer it: {combined}"
    );
    assert!(
        !combined.contains("input ended"),
        "the wizard ran and hit end-of-input: {combined}"
    );
    // The flag path has no key to store, so it must not claim to have stored
    // one - and must not write a store file at all.
    assert!(
        !combined.contains("Stored"),
        "nothing was pasted, so nothing was stored: {combined}"
    );
    assert!(
        !dir.path().join("auth.toml").exists(),
        "and no store file was created"
    );
}

#[test]
fn forcing_interactive_without_a_terminal_fails_rather_than_hanging() {
    // The other half of the same wiring. `--interactive` on a closed stdin has
    // no answers to read, and the only acceptable outcome is a prompt failure
    // that names what happened - never a hang, and never silently choosing
    // defaults nobody entered.
    let dir = repo();

    let output = init_with_closed_stdin(&dir, &["--interactive"]);

    assert!(!output.status.success(), "an unanswerable prompt must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("input ended"),
        "the failure should say why: {stderr}"
    );
    assert!(
        !dir.path().join("drep.toml").exists(),
        "and nothing was written from answers nobody gave"
    );
}

#[test]
fn a_forced_wizard_accepts_every_default_from_a_pipe() {
    // The other half of `Terminal::ask`: an empty line has to return the value
    // shown in brackets. Without that substitution the wizard would reject each
    // Enter as an empty answer and re-ask forever, which no in-process test can
    // see because `Terminal` reads the real stdin.
    //
    // Six answers, all empty: provider, endpoint, model, no fallback, hooks,
    // gitignore.
    let dir = repo();

    // Provider default, an explicit endpoint, then defaults for model, fallback,
    // hooks and gitignore.
    let output = run_init(
        &dir,
        &["--interactive"],
        Some(&format!("\n{DEAD}\n\n\n\n\n")),
    );

    assert!(
        output.status.success(),
        "init failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let config = std::fs::read_to_string(dir.path().join("drep.toml")).expect("config written");
    assert!(
        config.contains(DEAD),
        "the typed endpoint was used: {config}"
    );
    assert!(
        config.contains("qwen3-30b-a3b"),
        "and the offered model default: {config}"
    );
    assert!(
        dir.path().join(".gitignore").exists(),
        "and the gitignore question defaulted to yes"
    );
}

#[test]
fn a_pasted_key_reaches_the_store_and_the_config_omits_it() {
    // The whole point of the store, end to end through the real binary: the key
    // is written outside the repository, and `drep.toml` carries no `api_key`
    // line that would override it.
    //
    // Answers: the custom preset, the dead endpoint, the key, a model name, then
    // defaults for fallback, hooks and gitignore. The key is asked before the
    // model, because the model listing needs one to authenticate with.
    let dir = repo();

    let output = run_init(
        &dir,
        &["--interactive"],
        Some(&format!(
            "{}\n{DEAD}\nsk-integration-test\nsome-model\n\n\n\n",
            custom()
        )),
    );

    assert!(
        output.status.success(),
        "init failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        report.contains("Stored 1 key"),
        "the report should say a key was stored: {report}"
    );
    assert!(
        !report.contains("sk-integration-test"),
        "and must never echo it: {report}"
    );

    let store = std::fs::read_to_string(dir.path().join("auth.toml")).expect("store written");
    assert!(
        store.contains("sk-integration-test"),
        "the key reached the store: {store}"
    );
    assert!(store.contains(DEAD), "keyed by endpoint: {store}");

    let config = std::fs::read_to_string(dir.path().join("drep.toml")).expect("config written");
    assert!(
        !config
            .lines()
            .any(|line| line.trim_start().starts_with("api_key")),
        "no api_key assignment, which would override the stored key: {config}"
    );
    assert!(
        !config.contains("sk-integration-test"),
        "and the secret is nowhere near the repository: {config}"
    );
}

#[test]
fn re_running_on_a_configured_repo_changes_nothing_when_declined() {
    // The half-applied run this whole check exists to prevent: before it, the
    // second invocation asked every question, *stored the pasted key*, and then
    // failed on "drep.toml already exists" - leaving the store changed, the
    // config not, and the provider not switched.
    let dir = repo();

    let first = run_init(
        &dir,
        &["--interactive"],
        Some(&format!("\n{DEAD}\n\n\n\n\n")),
    );
    assert!(first.status.success(), "first init failed");
    let before = std::fs::read_to_string(dir.path().join("drep.toml")).expect("config");

    // Decline, then a full set of answers that would switch to z.ai and paste a
    // key if the run continued past the prompt.
    let second = run_init(
        &dir,
        &["--interactive"],
        Some(&format!(
            "n\n{}\n{DEAD}\nsk-should-not-be-stored\nsome-model\n\n\n\n",
            custom()
        )),
    );

    assert!(second.status.success(), "declining is not a failure");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("drep.toml")).expect("config"),
        before,
        "the config was left exactly as it was"
    );
    assert!(
        !dir.path().join("auth.toml").exists(),
        "and no key was stored from questions that were never reached"
    );
}

#[test]
fn re_running_and_accepting_switches_the_provider_and_stores_the_key() {
    let dir = repo();

    let first = run_init(
        &dir,
        &["--interactive"],
        Some(&format!("\n{DEAD}\n\n\n\n\n")),
    );
    assert!(first.status.success(), "first init failed");

    // Accept, then choose the custom preset and paste a key.
    let second = run_init(
        &dir,
        &["--interactive"],
        Some(&format!(
            "y\n{}\nhttp://127.0.0.1:9/switched\nsk-switched\nswitched-model\n\n\n\n",
            custom()
        )),
    );

    assert!(
        second.status.success(),
        "switch failed: {}{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let config = std::fs::read_to_string(dir.path().join("drep.toml")).expect("config");
    assert!(
        config.contains("http://127.0.0.1:9/switched"),
        "the provider was switched: {config}"
    );
    assert!(
        !config.contains("switched-model\nendpoint"),
        "and replaced rather than appended: {config}"
    );
    // By line: the file's own header comment carries a `[[llm]]` example, so
    // counting occurrences would count the documentation too.
    assert_eq!(
        config
            .lines()
            .filter(|line| line.trim() == "[[llm]]")
            .count(),
        1,
        "exactly one provider, not the old one plus the new: {config}"
    );

    let store = std::fs::read_to_string(dir.path().join("auth.toml")).expect("store");
    assert!(store.contains("sk-switched"), "and the key was stored");
}

#[test]
fn a_non_interactive_re_run_still_refuses_without_force() {
    // Scripts depend on this, and a script has nobody to ask.
    let dir = repo();
    let first = init_with_closed_stdin(&dir, &["--hooks", "none", "--no-gitignore"]);
    assert!(first.status.success(), "first init failed");

    let second = init_with_closed_stdin(&dir, &["--hooks", "none", "--no-gitignore"]);

    assert!(!second.status.success(), "a second run must refuse");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("--force"), "and name the flag: {stderr}");
}
