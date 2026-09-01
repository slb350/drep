use super::{common, rust_workflow};

fn remote_mutation_script() -> String {
    common::without_comments("scripts/mutants-remote.sh")
}

fn mutation_run_script() -> String {
    common::without_comments("scripts/mutants-run.sh")
}

fn mutation_common_script() -> String {
    common::without_comments("scripts/mutants-common.sh")
}

/// Mutation testing is an explicit local tool, not a hosted CI workload.
#[test]
fn mutation_testing_is_absent_from_github_ci() {
    let validation = rust_workflow();
    assert!(
        !validation.contains("\n  mutants:\n"),
        "the validation workflow must not run mutation testing"
    );
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflows/mutants.yml")
            .exists(),
        "a standalone mutation workflow would still trigger after main validation"
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
fn remote_mutation_sweep_defaults_to_homelab_2() {
    let script = remote_mutation_script();

    assert!(
        script.contains("HOST=\"${DREP_MUTANTS_HOST:-homelab-2.local}\""),
        "developer mutation offload must follow mutation ownership to homelab-2"
    );
    assert!(
        script.contains(
            "REMOTE_DIR=\"${DREP_MUTANTS_DIR:-.cache/drep-mutants/$(basename \"$PWD\")}\""
        ),
        "developer mutation offload must not collide with the protected runner checkout"
    );
    assert!(
        !script.contains("strix.local"),
        "deprecated Strix must never return as the mutation default"
    );
}

#[test]
fn remote_mutation_session_owns_sync_run_and_fresh_result_mirroring() {
    let script = remote_mutation_script();

    assert!(
        script.contains("DREP_MUTANTS_REMOTE_HOST_LOCK:-/srv/ci/drep-mutants/host.lock")
            && script.contains("exec 9>\"$host_lock\"")
            && script.contains("flock -E 75 -w \"$wait_seconds\" 9"),
        "developer and hosted mutation must share the homelab-2 host lock"
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
