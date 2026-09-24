use super::{common, rust_workflow, workflow_job};

fn mutation_workflow() -> String {
    common::without_comments(".github/workflows/mutants.yml")
}

fn remote_mutation_script() -> String {
    common::without_comments("scripts/mutants-remote.sh")
}

fn mutation_run_script() -> String {
    common::without_comments("scripts/mutants-run.sh")
}

fn mutation_common_script() -> String {
    common::without_comments("scripts/mutants-common.sh")
}

/// Trusted main pushes mutate only their production diff after validation.
///
/// The exhaustive sweep remains a scheduled and explicitly dispatched
/// backstop. It must not multiply all 1,490 mutants after every ordinary push,
/// and pull-request code must never reach the homelab runner.
#[test]
fn mutation_ci_splits_main_diff_checks_from_exhaustive_sweeps() {
    let validation = rust_workflow();
    let diff_mutants = workflow_job(&validation, "mutants-diff");
    assert!(
        diff_mutants.contains("needs: [linux, test-macos]")
            && diff_mutants
                .contains("if: github.event_name == 'push' && github.ref == 'refs/heads/main'")
            && diff_mutants
                .contains("runs-on: [self-hosted, linux, x64, homelab-ai-1, drep-mutants]"),
        "routine mutation must follow successful trusted validation on ai-1"
    );
    assert!(
        diff_mutants.contains("fetch-depth: 0") && !diff_mutants.contains("clean: false"),
        "the diff lane needs complete history and a clean checkout"
    );
    assert!(
        diff_mutants.contains("tool: cargo-mutants@27.1.0")
            && diff_mutants.contains("components: clippy")
            && diff_mutants.contains("PUSH_BASE: ${{ github.event.before }}")
            && diff_mutants.contains("PUSH_HEAD: ${{ github.sha }}")
            && diff_mutants.contains(
                "git diff --no-ext-diff --unified=0 \"$PUSH_BASE\" \"$PUSH_HEAD\" > \"$RUNNER_TEMP/pushed.diff\""
            )
            && diff_mutants.contains(
                "./scripts/mutants-run.sh --in-diff \"$RUNNER_TEMP/pushed.diff\""
            ),
        "routine CI must materialize and run the shared verdict over the complete pushed diff"
    );

    let workflow = mutation_workflow();
    let trigger = workflow
        .split_once("\njobs:")
        .map(|(trigger, _)| trigger)
        .expect("the mutation workflow must declare jobs");
    assert!(
        trigger.contains("workflow_dispatch:")
            && trigger.contains("schedule:")
            && trigger.contains("cron:"),
        "exhaustive mutation must be scheduled and manually dispatchable"
    );
    assert!(
        !trigger.contains("workflow_run:")
            && !trigger.contains("pull_request")
            && !trigger.contains("\n  push:"),
        "ordinary pushes and pull requests must not start an exhaustive sweep"
    );

    let mutants = workflow_job(&workflow, "mutants");
    assert!(
        mutants.contains(
            "if: github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"
        ) && mutants.contains("ref: ${{ github.sha }}"),
        "manual full sweeps must fail closed outside the default branch and use the triggering SHA"
    );
    assert!(
        mutants.contains("runs-on: [self-hosted, linux, x64, homelab-ai-1, drep-mutants]"),
        "the full sweep must require the dedicated homelab-ai-1 mutation label"
    );
    let timeout_lines = mutants
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("timeout-minutes:"))
        .collect::<Vec<_>>();
    assert_eq!(
        timeout_lines,
        ["timeout-minutes: 420"],
        "the dedicated mutation sweep needs one exact measured timeout while still releasing a wedged runner"
    );
    assert!(
        mutants.contains("tool: cargo-mutants@27.1.0"),
        "the mutation gate must retain its verified cargo-mutants version"
    );
    assert!(
        mutants.contains("components: clippy"),
        "the mutation runner must install Clippy because the suite exercises configured Rust compilers"
    );
    assert!(
        !mutants.contains("clean: false"),
        "cargo-mutants builds each mutant in a copy without target/, so the sweep starts from a clean checkout"
    );
    assert!(
        mutants.contains("./scripts/mutants-run.sh"),
        "exhaustive CI and local hooks must share one mutation verdict implementation"
    );
}

/// A no-argument full sweep must remain a genuinely empty cargo-mutants scope.
///
/// Expanding `"$@"` as part of the remote-command argument loop contributes
/// zero words when the caller supplied no scope. The remote script shifts only
/// its five transport fields, leaving a genuinely empty argument vector for
/// cargo-mutants.
#[test]
fn remote_full_mutation_sweep_passes_no_phantom_argument() {
    let script = remote_mutation_script();

    assert!(
        script.contains("for remote_arg in")
            && script.contains("shift 5")
            && script.contains("./scripts/mutants-run.sh \"$@\""),
        "the remote wrapper must preserve an empty post-transport argument vector"
    );
    assert!(
        !script.contains("$(printf '%q ' \"$@\")"),
        "empty positional parameters must not be formatted into a literal empty argument"
    );
}

#[test]
fn remote_mutation_sweep_defaults_to_bounded_ai1() {
    let script = remote_mutation_script();

    assert!(
        script.contains("HOST=\"${DREP_MUTANTS_HOST:-steve@192.168.68.88}\""),
        "developer mutation offload must follow hosted mutation ownership to ai-1"
    );
    assert!(
        script.contains("AI1_CI_ROLE=drep-mutants")
            && script.contains("REMOTE_DIR=\"$(remote_checkout_dir \"$AI1_CI_ROLE\")\""),
        "developer mutation offload must use this checkout's own directory in the role's cache"
    );
    assert!(
        !script.contains("homelab-2.local") && !script.contains("strix.local"),
        "retired mutation hosts must never return as the offload default"
    );
}

#[test]
fn remote_mutation_session_owns_sync_run_and_fresh_result_mirroring() {
    let script = remote_mutation_script();

    assert!(
        script.contains("exec 9>>\"${DREP_MUTANTS_HOST_LOCK:?")
            && script.contains("flock -E 75 -w \"$wait_seconds\" 9")
            && !script.contains("unset DREP_MUTANTS_HOST_LOCK"),
        "the offloaded run must take the role's host lock, the one hosted sweeps take, and hand it to the run on descriptor 9"
    );
    assert!(
        script.contains("\"$MUTANTS_HOST_LOCK_WAIT_SECONDS\"")
            && script.contains("DREP_MUTANTS_RSYNC_TIMEOUT_SECONDS")
            && script.contains("--timeout=\"$RSYNC_IO_TIMEOUT_SECONDS\""),
        "remote lock and transfer waits must remain explicitly bounded"
    );
    assert!(
        script.contains("mkfifo \"$CONTROL_IN\" \"$CONTROL_OUT\"")
            && script.contains("mutants-lock-ready:$RUN_TOKEN")
            && script.contains("mutants-run-finished:$RUN_TOKEN")
            && script.contains("DREP_MUTANTS_RESULT_TOKEN")
            && script.contains(".run-token")
            && script.contains("printf 'mirrored\\n'"),
        "one remote lock session must prove that mirrored results belong to the current run"
    );
    assert!(
        script.contains("kill \"$REMOTE_SESSION_PID\"")
            && script.contains("wait \"$REMOTE_SESSION_PID\"")
            && script.contains("trap 'exit 74' PIPE"),
        "abnormal local exit, a dead session included, must terminate and reap the remote lock session"
    );
    let session_start = script
        .find("REMOTE_SESSION_PID=$!")
        .expect("remote session PID assignment must exist");
    let source_sync = script
        .find("rsync -a --delete")
        .expect("source synchronization must exist");
    assert!(
        session_start < source_sync,
        "the host lock must be acquired before source synchronization begins"
    );
}

/// The checkout lock is taken before the host is probed, so a run that waited
/// for it does not act on a probe that is half an hour old.
#[test]
fn remote_mutation_takes_the_checkout_lock_before_probing_the_host() {
    let script = remote_mutation_script();
    let lock = script
        .find("acquire_checkout_lock mutants-remote")
        .expect("checkout lock");
    let probe = script
        .find("ssh -o BatchMode=yes -o ConnectTimeout=5")
        .expect("host probe");
    assert!(lock < probe);
}

/// The source sync mirrors this checkout with --delete, so its remote
/// directory is named for this machine and the checkout's path: two checkouts
/// never share one, and holding the checkout lock is all it takes to own it.
#[cfg(unix)]
#[test]
fn checkouts_with_one_name_get_their_own_remote_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake_bin = temp.path().join("bin");
    std::fs::create_dir_all(&fake_bin).expect("fake bin");
    common::write_executable(
        &fake_bin.join("hostname"),
        "#!/bin/sh\necho other-machine\n",
    );
    let remote_dir_on = |parent: &str, name: &str, machine: Option<&std::path::Path>| {
        let scripts = temp.path().join(parent).join(name).join("scripts");
        std::fs::create_dir_all(&scripts).expect("scripts directory");
        std::fs::copy(
            format!("{}/scripts/mutants-common.sh", env!("CARGO_MANIFEST_DIR")),
            scripts.join("mutants-common.sh"),
        )
        .expect("copy mutation script");
        let mut command = std::process::Command::new("bash");
        command
            .args([
                "-c",
                ". \"$1/mutants-common.sh\" && remote_checkout_dir drep-mutants",
                "remote-dir-test",
            ])
            .arg(&scripts)
            .current_dir(temp.path());
        if let Some(bin) = machine {
            command.env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")),
            );
        }
        let output = command.output().expect("derive the remote directory");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).expect("utf-8 directory")
    };
    let remote_dir = |parent: &str, name: &str| remote_dir_on(parent, name, None);

    let first = remote_dir("one", "drep");
    let second = remote_dir("two", "drep");
    let odd = remote_dir("three", "my repo;x");

    for dir in [&first, &second, &odd] {
        let name = dir
            .strip_prefix(".cache/drep-mutants/")
            .unwrap_or_else(|| panic!("{dir} must sit in the role's cache"));
        assert!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                && name != "."
                && name != "..",
            "{dir} must be one plain directory name"
        );
    }
    assert!(first.starts_with(".cache/drep-mutants/drep-"));
    assert!(odd.starts_with(".cache/drep-mutants/myrepox-"));
    assert_ne!(
        first, second,
        "same-named checkouts must not share a directory"
    );
    assert_eq!(
        first,
        remote_dir("one", "drep"),
        "a checkout keeps its directory"
    );
    assert_ne!(
        first,
        remote_dir_on("one", "drep", Some(&fake_bin)),
        "the same path on another machine must not share a directory"
    );
}

/// A fixture repository with the staged wrapper, a fake remote that records how
/// it was called, and one committed Rust file.
#[cfg(unix)]
fn staged_fixture(temp: &std::path::Path) -> std::path::PathBuf {
    let repository = temp.join("repository");
    let scripts = repository.join("scripts");
    std::fs::create_dir_all(&scripts).expect("scripts directory");
    for name in ["mutants-common.sh", "mutants-staged.sh"] {
        std::fs::copy(
            format!("{}/scripts/{name}", env!("CARGO_MANIFEST_DIR")),
            scripts.join(name),
        )
        .expect("copy mutation script");
    }
    // Records whether another process is refused the checkout lock, and whether
    // this process, started by the lock's holder, gets it without waiting.
    common::write_executable(
        &scripts.join("mutants-remote.sh"),
        "#!/usr/bin/env bash\nperl -MFcntl=:flock -e \"$LOCK_PROBE\" target/mutants.lock; refused=$?\n. scripts/mutants-common.sh\nMUTANTS_HOST_LOCK_WAIT_SECONDS=0\nacquire_checkout_lock fake-remote; inherited=$?\nprintf '%s|%s|%s|%s\\n' \"$*\" \"$MUTANTS_EXTRA_FILES\" \"$inherited\" \"$refused\" >> \"$FAKE_EVENTS\"\n",
    );
    std::fs::write(repository.join(".gitignore"), "target/\n").expect("gitignore");
    std::fs::write(repository.join("lib.rs"), "fn one() {}\n").expect("source");
    staged_git(&repository, &["init", "-q", "-b", "main"]);
    staged_git(&repository, &["add", "."]);
    staged_git(
        &repository,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    repository
}

#[cfg(unix)]
fn staged_git(repository: &std::path::Path, arguments: &[&str]) {
    let output = common::without_outer_git("git", repository)
        .args(arguments)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {arguments:?}: {output:?}");
}

#[cfg(unix)]
fn run_staged(repository: &std::path::Path, events: &std::path::Path) -> std::process::Output {
    common::without_outer_git("bash", repository)
        .arg("scripts/mutants-staged.sh")
        .env("FAKE_EVENTS", events)
        .env("LOCK_PROBE", common::LOCK_PROBE)
        .env("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "0")
        .output()
        .expect("run the staged wrapper")
}

/// A staged run holds the checkout lock from writing its diff until the remote
/// run returns, so a manual sweep in the same checkout cannot overwrite the diff
/// or the results in between.
#[cfg(unix)]
#[test]
fn staged_run_holds_the_checkout_lock_across_the_remote_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repository = staged_fixture(temp.path());
    let events = temp.path().join("events");
    std::fs::write(repository.join("lib.rs"), "fn two() {}\n").expect("staged source");
    staged_git(&repository, &["add", "lib.rs"]);

    let output = run_staged(&repository, &events);

    assert!(output.status.success(), "{output:?}");
    let call = std::fs::read_to_string(&events).expect("remote call");
    let fields = call.trim_end().split('|').collect::<Vec<_>>();
    let diff = "target/mutants/staged.diff";
    assert_eq!(fields[0], format!("--in-diff {diff}"));
    assert_eq!(fields[1], diff, "the diff must be named for the transfer");
    assert_eq!(
        fields[2], "0",
        "the remote run must inherit the lock instead of waiting on it"
    );
    assert_eq!(
        fields[3], "1",
        "another process must be refused the lock while the remote run works"
    );
    assert!(
        std::fs::read_to_string(repository.join(diff))
            .expect("staged diff")
            .contains("+fn two() {}")
    );
    assert!(
        common::lock_is_free(&repository.join("target/mutants.lock")),
        "the lock must be free once the staged run returns"
    );
}

/// A commit with no Rust changes has nothing to mutate, so it leaves at once
/// even while a sweep holds this checkout's lock.
#[cfg(unix)]
#[test]
fn a_commit_without_rust_changes_does_not_wait_for_a_running_sweep() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repository = staged_fixture(temp.path());
    let events = temp.path().join("events");
    std::fs::write(repository.join("notes.md"), "notes\n").expect("staged prose");
    staged_git(&repository, &["add", "notes.md"]);
    std::fs::create_dir_all(repository.join("target")).expect("target");
    let mut sweep = common::hold_lock(&repository.join("target/mutants.lock"), "30");

    let output = run_staged(&repository, &events);
    let _ = sweep.kill();
    let _ = sweep.wait();

    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no staged Rust changes"),
        "{output:?}"
    );
    assert!(!events.exists(), "nothing may reach the remote");
}

#[test]
fn mutation_runner_holds_the_configured_host_lock() {
    let script = mutation_run_script();

    assert!(
        script.contains("HOST_LOCK=\"${DREP_MUTANTS_HOST_LOCK:-}\"")
            && script.contains("hold_lock 9 \"$HOST_LOCK\" mutants-run"),
        "a configured mutation host must serialize GitHub and laptop-offloaded sweeps"
    );
    assert!(
        script.contains("DREP_MUTANTS_RESULT_TOKEN")
            && script.contains("$OUT_DIR/mutants.out")
            && script.contains("$OUT_DIR/.run-token"),
        "each remote run must clear stale output and publish its own freshness token"
    );
    assert!(
        script.contains("\"$@\" 6<&- 9<&- && status=0"),
        "cargo-mutants and its fixtures must not inherit the checkout or host lock"
    );
}

#[test]
fn mutation_host_lock_wait_policy_has_one_definition() {
    let common = mutation_common_script();
    assert!(
        common.contains(
            "MUTANTS_HOST_LOCK_WAIT_SECONDS=\"${DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS:-1800}\""
        ) && common.contains("validate_mutants_host_lock_wait_seconds()")
            && common.contains("validate_mutants_host_lock_wait_seconds \"$caller\" || return"),
        "the shared mutation layer must own the lock wait default and check it before every lock"
    );

    for (name, script) in [
        ("mutants-remote", remote_mutation_script()),
        ("mutants-run", mutation_run_script()),
    ] {
        assert!(
            !script.contains(
                "HOST_LOCK_WAIT_SECONDS=\"${DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS:-1800}\""
            ),
            "{name} must not redefine the shared host-lock wait policy"
        );
    }
}

#[test]
fn ai1_transport_fails_closed_without_bypassing_the_sandbox() {
    let output = std::process::Command::new("bash")
        .arg("tests/ai1-transport.sh")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("transport contract must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
