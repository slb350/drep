//! End-to-end exit-code contract.
//!
//! These numbers are what git hooks and CI branch on, so they are checked by
//! spawning the real binary. The unit test in `lib.rs` pins `Exit::code()`,
//! but the mapping that decides a hook's fate - error to exit 2, success to
//! the returned code - lives in `main.rs` and is only observable from outside
//! the process.

use assert_cmd::Command;
use tempfile::TempDir;

fn drep() -> Command {
    let mut command = Command::cargo_bin("drep").expect("binary builds");
    // Machine-level policy, pointed at a file nothing creates. A developer or a
    // runner carrying a real fleet policy would otherwise see this suite behave
    // differently from CI - and a policy naming `refuse_markers` would make every
    // `check` here fail closed, since none of these fixtures is a repository. The
    // one test that wants a policy sets the variable again, and the later value
    // wins.
    command.env("DREP_SITE_CONFIG", absent_site_policy());
    command
}

/// A path no test writes, in a directory only this user can write.
///
/// Not `std::env::temp_dir()`: on Linux that is world-writable `/tmp`, so a
/// leftover file or another user creating one predictable name would supply a real
/// policy to this whole suite - garbage content exits 2 through
/// `SiteConfigError::Parse`, and `refuse_markers` exits 2 through
/// `MarkerRootUnresolved`, since none of these fixtures is a repository.
/// `CARGO_TARGET_TMPDIR` is cargo's own per-crate scratch directory for
/// integration tests, and lives under `target/`.
///
/// Not the empty string either: `DREP_SITE_CONFIG=` deliberately falls back to the
/// machine path rather than switching enforcement off, so an empty override would
/// isolate nothing. Nor does any override displace a policy actually installed at
/// the machine path - on a machine carrying one, these tests read it, which is the
/// layer working as designed.
fn absent_site_policy() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("absent-site-policy.toml")
}

/// A directory holding one file, for the `lint-docs` and `check` cases.
fn repo_with(name: &str, content: &str) -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join(name), content).expect("write");
    dir
}

/// Give `dir` a minimal config, so `check` gets past loading one.
///
/// The endpoint is deliberately dead. Every assertion below is about input
/// resolution, which runs before any request - and a test that needed a live
/// model to prove an argument was rejected would be testing the model.
fn with_config(dir: &TempDir) {
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nmodel = \"m\"\nendpoint = \"http://127.0.0.1:59999/v1\"\n",
    )
    .expect("write config");
}

#[test]
fn version_succeeds() {
    drep().arg("--version").assert().success();
}

#[test]
fn help_succeeds() {
    drep().arg("--help").assert().success();
}

#[test]
fn lint_docs_is_report_only_by_default_and_gates_under_strict() {
    // The three exit codes of `lint-docs`, through the real binary, because
    // the mapping from `Exit` to a process status lives in `main.rs`.
    let dir = repo_with("bad.md", "#Heading\n");
    drep()
        .arg("lint-docs")
        .current_dir(dir.path())
        .assert()
        .code(0);
    drep()
        .args(["lint-docs", "--strict"])
        .current_dir(dir.path())
        .assert()
        .code(1);

    let clean = repo_with("good.md", "# Heading\n\nprose\n");
    drep()
        .args(["lint-docs", "--strict"])
        .current_dir(clean.path())
        .assert()
        .code(0)
        .stdout("No issues found.\n");
}

#[test]
fn a_named_file_no_command_can_analyze_never_exits_clean() {
    // The failure a commit gate must never have: exiting 0 without analyzing.
    // Both directions of the file-class split, through the real binary.
    let dir = repo_with("README.md", "# T\n\nprose\n");
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("write");
    with_config(&dir);

    let assert = drep()
        .args(["check", "README.md"])
        .current_dir(dir.path())
        .assert()
        .code(2);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains("lint-docs"), "{stdout}");

    let assert = drep()
        .args(["lint-docs", "main.rs"])
        .current_dir(dir.path())
        .assert()
        .code(2);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains("drep check"), "{stdout}");
}

#[test]
fn an_unparseable_site_policy_exits_two_through_the_real_binary() {
    // The only test that exercises the environment read in
    // `config::site::default_path` together with `main.rs`'s error-to-exit-2
    // mapping. `assert_cmd` sets the variable on the child alone, which is how
    // this reaches code no in-process test can safely touch: `std::env::set_var`
    // is unsafe in edition 2024 and the test process is multi-threaded.
    let dir = repo_with("lib.py", "x = 1\n");
    with_config(&dir);
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "not toml at all\n").expect("write site policy");

    let assert = drep()
        .args(["check", "lib.py"])
        .current_dir(dir.path())
        .env("DREP_SITE_CONFIG", &site)
        .assert()
        .code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8");
    assert!(
        stderr.contains(&site.display().to_string()),
        "a policy drep could not load is never a clean or a merely-blocked run, \
         and the message has to name the file; got {stderr}"
    );
}

#[test]
fn usage_error_also_blocks() {
    // clap exits 2 on a usage error, colliding with "could not analyze". Both
    // mean "do not let this commit through", so the collision is safe - this
    // test records that it is deliberate rather than unnoticed.
    drep().arg("no-such-command").assert().code(2);
    drep()
        .args(["check", "--fail-on", "critical"])
        .assert()
        .code(2);
    drep().args(["check", "--staged", "a.rs"]).assert().code(2);
}

#[test]
fn no_command_is_a_usage_error() {
    drep().assert().failure();
}
