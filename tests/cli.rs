//! End-to-end exit-code contract.
//!
//! These numbers are what git hooks and CI branch on, so they are checked by
//! spawning the real binary. The unit test in `lib.rs` pins `Exit::code()`,
//! but the mapping that decides a hook's fate - error to exit 2, success to
//! the returned code - lives in `main.rs` and is only observable from outside
//! the process.

use assert_cmd::Command;

fn drep() -> Command {
    Command::cargo_bin("drep").expect("binary builds")
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
fn unimplemented_command_exits_unanalyzed_not_clean() {
    // The failure a commit gate must never have: exiting 0 without analyzing.
    for command in ["check", "lint-docs", "doctor", "init"] {
        drep().arg(command).assert().code(2);
    }
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
