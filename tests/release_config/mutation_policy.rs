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
        diff_mutants.contains("fetch-depth: 0")
            && diff_mutants.contains("clean: false")
            && diff_mutants
                .contains("git status --porcelain=v1 --untracked-files=all --ignored=matching")
            && diff_mutants.contains("^!! target/$"),
        "the diff lane needs complete history and a fail-closed warm workspace"
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
        mutants.contains("clean: false")
            && mutants
                .contains("git status --porcelain=v1 --untracked-files=all --ignored=matching")
            && mutants.contains("^!! target/$"),
        "the warm target cache must be retained only behind a fail-closed workspace check"
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
/// its six transport fields, leaving a genuinely empty argument vector for
/// cargo-mutants.
#[test]
fn remote_full_mutation_sweep_passes_no_phantom_argument() {
    let script = remote_mutation_script();

    assert!(
        script.contains("for remote_arg in")
            && script.contains("shift 6")
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
        script.contains(
            "REMOTE_DIR=\"${DREP_MUTANTS_DIR:-.cache/drep-mutants/$(basename \"$PWD\")}\""
        ),
        "developer mutation offload must not collide with the protected runner checkout"
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
        script.contains("DREP_MUTANTS_REMOTE_HOST_LOCK:-/srv/ci/fleet/drep-mutants/home/host.lock")
            && script.contains("exec 9>\"$host_lock\"")
            && script.contains("flock -E 75 -w \"$wait_seconds\" 9"),
        "developer and hosted mutation must share the ai-1 host lock"
    );
    assert!(
        script.contains("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS")
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
            && script.contains("wait \"$REMOTE_SESSION_PID\""),
        "abnormal local exit must terminate and reap the remote lock session"
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

/// The source sync mirrors with --delete, so the destination must be a path
/// below the remote home that can only be this checkout's.
#[test]
fn remote_mutation_refuses_unsafe_remote_directories_before_connecting() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("fake bin");
    let contacted = temp.path().join("contacted");
    common::write_executable(
        &bin.join("ssh"),
        "#!/bin/sh\n: >\"$FAKE_CONTACTED\"\nexit 1\n",
    );
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"));

    for unsafe_dir in [
        ".",
        "..",
        "../elsewhere",
        "/abs/path",
        "cache/../..",
        "cache/.",
        "./cache",
    ] {
        let output = std::process::Command::new("bash")
            .arg("scripts/mutants-remote.sh")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("PATH", &path)
            .env("FAKE_CONTACTED", &contacted)
            .env("DREP_MUTANTS_DIR", unsafe_dir)
            .env("DREP_MUTANTS_HOST", "steve@192.168.68.88")
            .env_remove("DREP_MUTANTS_REMOTE")
            .output()
            .expect("run the remote wrapper");
        assert_eq!(
            output.status.code(),
            Some(64),
            "{unsafe_dir:?} must be refused: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !contacted.exists(),
            "{unsafe_dir:?} must be refused before any SSH"
        );
    }
}

/// A remote directory someone else populated is refused, and every transfer
/// stops if the session holding the host lock ends.
#[test]
fn remote_session_refuses_foreign_destinations_and_guards_every_transfer() {
    let script = remote_mutation_script();

    assert!(
        script.contains("marker=\"$checkout/.mutants-remote-checkout\"")
            && script.contains(
                "[[ ! -f $marker && ! -f $checkout/scripts/mutants-remote.sh && -n $(ls -A \"$checkout\") ]]"
            )
            && script.contains("exit 78")
            && script.contains("--filter='P /.mutants-remote-checkout'"),
        "the session must refuse a non-empty directory it did not create, and the sync must keep its marker"
    );
    let marker_check = script
        .find("marker=\"$checkout/.mutants-remote-checkout\"")
        .expect("marker check");
    let lock_ready = script
        .find("printf \"mutants-lock-ready:%s\\n\"")
        .expect("lock-ready handshake");
    assert!(
        marker_check < lock_ready,
        "the destination is judged after the lock is held and before any transfer starts"
    );
    for transfer in [
        "while_session_holds_lock rsync -a --delete",
        "while_session_holds_lock rsync -aR --timeout",
        "while_session_holds_lock rsync -a --timeout",
    ] {
        assert!(
            script.contains(transfer),
            "{transfer} must stop with the lock session"
        );
    }
    assert!(
        script.contains("if ! kill -0 \"$REMOTE_SESSION_PID\"")
            && script.contains("pkill -TERM -P \"$transfer\""),
        "a transfer must be stopped, child rsync included, once its lock session is gone"
    );
}

/// A staged run holds the checkout lock from writing its diff until the remote
/// run returns, so a manual sweep in the same checkout cannot overwrite the diff
/// or the results in between.
#[cfg(unix)]
#[test]
fn staged_run_holds_the_checkout_lock_across_the_remote_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repository = temp.path().join("repository");
    let scripts = repository.join("scripts");
    std::fs::create_dir_all(&scripts).expect("scripts directory");
    for name in ["mutants-common.sh", "mutants-staged.sh"] {
        std::fs::copy(
            format!("{}/scripts/{name}", env!("CARGO_MANIFEST_DIR")),
            scripts.join(name),
        )
        .expect("copy mutation script");
    }
    let events = temp.path().join("events");
    common::write_executable(
        &scripts.join("mutants-remote.sh"),
        "#!/usr/bin/env bash\nprintf '%s|%s|%s|%s\\n' \"$*\" \"$MUTANTS_EXTRA_FILES\" \"${MUTANTS_CHECKOUT_LOCK_HELD:-}\" \"$(cat target/mutants.lock/pid 2>/dev/null)\" >> \"$FAKE_EVENTS\"\n",
    );
    let git = |arguments: &[&str]| {
        let output = common::without_outer_git("git", &repository)
            .args(arguments)
            .output()
            .expect("run git");
        assert!(output.status.success(), "git {arguments:?}: {output:?}");
    };
    std::fs::write(repository.join(".gitignore"), "target/\n").expect("gitignore");
    std::fs::write(repository.join("lib.rs"), "fn one() {}\n").expect("source");
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "fixture",
    ]);
    std::fs::write(repository.join("lib.rs"), "fn two() {}\n").expect("staged source");
    git(&["add", "lib.rs"]);

    let output = common::without_outer_git("bash", &repository)
        .arg("scripts/mutants-staged.sh")
        .env("FAKE_EVENTS", &events)
        .env_remove("MUTANTS_CHECKOUT_LOCK_HELD")
        .output()
        .expect("run the staged wrapper");

    assert!(output.status.success(), "{output:?}");
    let call = std::fs::read_to_string(&events).expect("remote call");
    let fields = call.trim_end().split('|').collect::<Vec<_>>();
    let diff = "target/mutants/staged.diff";
    assert_eq!(fields[0], format!("--in-diff {diff}"));
    assert_eq!(fields[1], diff, "the diff must be named for the transfer");
    assert_eq!(
        fields[2], "1",
        "the remote run must inherit the lock instead of waiting on it"
    );
    assert!(
        !fields[3].is_empty(),
        "the lock must be held while the remote run works"
    );
    assert!(
        std::fs::read_to_string(repository.join(diff))
            .expect("staged diff")
            .contains("+fn two() {}")
    );
    assert!(
        !repository.join("target/mutants.lock").exists(),
        "the staged run must release the lock when the remote run returns"
    );
}

#[test]
fn mutation_runner_holds_the_configured_host_lock() {
    let script = mutation_run_script();

    assert!(
        script.contains("DREP_MUTANTS_HOST_LOCK")
            && script.contains("validate_mutants_host_lock_wait_seconds mutants-run")
            && script.contains("MUTANTS_HOST_LOCK_WAIT_SECONDS")
            && script.contains("flock -w")
            && script.contains("exec 9>\"$HOST_LOCK\""),
        "a configured mutation host must serialize GitHub and laptop-offloaded sweeps"
    );
    assert!(
        script.contains("DREP_MUTANTS_RESULT_TOKEN")
            && script.contains("$OUT_DIR/mutants.out")
            && script.contains("$OUT_DIR/.run-token"),
        "each remote run must clear stale output and publish its own freshness token"
    );
}

#[test]
fn mutation_host_lock_wait_policy_has_one_definition() {
    let common = mutation_common_script();
    assert!(
        common.contains(
            "MUTANTS_HOST_LOCK_WAIT_SECONDS=\"${DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS:-1800}\""
        ) && common.contains("validate_mutants_host_lock_wait_seconds()"),
        "the shared mutation layer must own the host-lock wait default and validation"
    );

    for (name, script) in [
        ("mutants-remote", remote_mutation_script()),
        ("mutants-run", mutation_run_script()),
    ] {
        assert!(
            script.contains(&format!("validate_mutants_host_lock_wait_seconds {name}")),
            "{name} must invoke the shared host-lock wait validator"
        );
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
