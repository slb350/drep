use super::common;

/// Runs `mutants-run.sh` against `scratch` with a cargo that records its
/// arguments and TMPDIR and plants copies the run must clean up. `prelude` runs
/// in the same shell first, so it can hand the script an open descriptor.
#[cfg(unix)]
fn run_with_fake_cargo(
    temp: &std::path::Path,
    scratch: &std::path::Path,
    extra_env: &[(&str, &std::ffi::OsStr)],
    prelude: &str,
) -> std::process::Output {
    let script = format!("{}/scripts/mutants-run.sh", env!("CARGO_MANIFEST_DIR"));
    let mut command = std::process::Command::new("bash");
    command
        .args([
            "-c",
            &format!(
                "cargo() {{ mkdir -p \"$TMPDIR/cargo-mutants-trap.tmp/nested\" \"$TMPDIR/.tmp-test-debris\"; printf '%s\\n' \"$@\" >\"$DREP_MUTANTS_CAPTURE_ARGS\"; printf '%s\\n' \"$TMPDIR\" >\"$DREP_MUTANTS_CAPTURE_TMPDIR\"; }}; export -f cargo; {prelude}\"$1\""
            ),
            "mutation-cleanup-test",
            &script,
        ])
        .env("DREP_MUTANTS_TMPDIR", scratch)
        .env("DREP_MUTANTS_CAPTURE_ARGS", temp.join("cargo-args"))
        .env("DREP_MUTANTS_CAPTURE_TMPDIR", temp.join("cargo-tmpdir"))
        .env("MUTANTS_OUT_DIR", temp.join("out"))
        .env_remove("DREP_MUTANTS_HOST_LOCK")
        .env_remove("DREP_MUTANTS_RESULT_TOKEN");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .output()
        .expect("run mutation wrapper with fake cargo")
}

/// Mutation scratch copies live beside the checkout, never in the system temp dir.
#[test]
fn mutation_scratch_copies_stay_off_the_tmpfs() {
    let script = common::without_comments("scripts/mutants-run.sh");

    assert!(
        script.contains("RUN_SCRATCH=\"${DREP_MUTANTS_TMPDIR:-${MUTANTS_ROOT}.mutants-tmp}/run\"")
            && script.contains("export TMPDIR=\"$RUN_SCRATCH\""),
        "the run must place its scratch copies in its directory beside the checkout"
    );
    assert!(
        !script
            .lines()
            .any(|line| line.contains("TMPDIR=") && line.contains("/tmp")),
        "scratch copies must never default to the system temp dir"
    );
    assert!(
        !script.contains("rm ") && !script.contains("rmdir "),
        "mutation cleanup must never invoke rm or rmdir"
    );
    assert!(
        script.contains("trap 'remove_tree \"$RUN_SCRATCH\"' EXIT"),
        "the run must remove its own copies on exit"
    );
}

/// The sweep is destructive only inside the run directory, and the run removes
/// that directory, test debris included, when it ends.
#[cfg(unix)]
#[test]
fn mutation_scratch_cleanup_preserves_adjacent_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let stale_run = scratch.join("run/cargo-mutants-killed.tmp/nested");
    let adjacent = scratch.join("adjacent");
    for directory in [&stale_run, &adjacent] {
        std::fs::create_dir_all(directory).expect("seed directory");
    }
    std::fs::write(adjacent.join("keep"), "keep").expect("adjacent file");

    let output = run_with_fake_cargo(temp.path(), &scratch, &[], "");

    assert!(
        output.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !scratch.join("run").exists(),
        "the run must remove its directory, stale copies and test debris included"
    );
    assert!(adjacent.join("keep").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("cargo-tmpdir"))
            .expect("captured TMPDIR")
            .trim_end(),
        scratch.join("run").to_str().expect("utf-8 path"),
        "cargo must see the run directory as TMPDIR"
    );
    assert!(
        common::lock_is_free(&temp.path().join("out.lock")),
        "the run must release the checkout lock"
    );

    let args =
        std::fs::read_to_string(temp.path().join("cargo-args")).expect("captured cargo arguments");
    let timeout_values = args
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0] == "--minimum-test-timeout")
        .map(|pair| pair[1])
        .collect::<Vec<_>>();
    assert_eq!(
        timeout_values,
        ["120"],
        "the executed mutation command needs one exact test-timeout floor above the observed 60-second capacity false positives"
    );
}

/// A run directory that is a symlink is removed as a link; what it points at
/// is left alone.
#[cfg(unix)]
#[test]
fn a_symlinked_run_directory_is_not_followed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&scratch).expect("scratch");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("keep"), "keep").expect("outside file");
    std::os::unix::fs::symlink(&outside, scratch.join("run")).expect("run symlink");

    let output = run_with_fake_cargo(temp.path(), &scratch, &[], "");

    assert!(output.status.success(), "{output:?}");
    assert!(outside.join("keep").exists());
    assert!(!scratch.join("run").exists());
}

/// A second run in the same checkout waits for the first rather than deleting
/// its results or copies, and gives up with 75 instead of running.
#[cfg(unix)]
#[test]
fn a_second_run_in_one_checkout_waits_for_the_first() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let mut first = common::hold_lock(&temp.path().join("out.lock"), "30");
    let results = temp.path().join("out/mutants.out");
    std::fs::create_dir_all(&results).expect("first run's results");
    std::fs::write(results.join("missed.txt"), "first run's survivor\n")
        .expect("first run's verdict");

    let output = run_with_fake_cargo(
        temp.path(),
        &scratch,
        &[("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "0".as_ref())],
        "",
    );
    let _ = first.kill();
    let _ = first.wait();

    assert_eq!(output.status.code(), Some(75), "{output:?}");
    assert!(
        !temp.path().join("cargo-args").exists(),
        "the second run must not start cargo-mutants"
    );
    assert_eq!(
        std::fs::read_to_string(results.join("missed.txt")).expect("first run's verdict"),
        "first run's survivor\n",
        "the second run must not touch the first run's results"
    );
}

/// A run that is allowed to wait takes the lock once its holder lets go.
#[cfg(unix)]
#[test]
fn a_waiting_run_starts_once_the_lock_is_released() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = common::hold_lock(&temp.path().join("out.lock"), "0.2");

    let output = run_with_fake_cargo(
        temp.path(),
        &temp.path().join("scratch"),
        &[("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "30".as_ref())],
        "",
    );
    let _ = first.wait();

    assert!(output.status.success(), "{output:?}");
    assert!(temp.path().join("cargo-args").exists());
}

/// The kernel drops the lock with its holder, so a lock file a killed run left
/// behind holds nothing.
#[cfg(unix)]
#[test]
fn a_leftover_lock_file_holds_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let lock = temp.path().join("out.lock");
    std::fs::write(&lock, "").expect("leftover lock file");

    let output = run_with_fake_cargo(temp.path(), &scratch, &[], "");

    assert!(output.status.success(), "{output:?}");
    assert!(
        temp.path().join("cargo-args").exists(),
        "the run must go ahead"
    );
    assert!(common::lock_is_free(&lock), "the run must release the lock");
}

/// The remote session holds the host lock on descriptor 9 and starts the run
/// with it open: the run carries on under that lock instead of waiting on it.
#[cfg(unix)]
#[test]
fn a_run_started_by_the_host_lock_holder_reuses_its_lock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let host_lock = temp.path().join("host.lock");

    let output = run_with_fake_cargo(
        temp.path(),
        &temp.path().join("scratch"),
        &[
            ("DREP_MUTANTS_HOST_LOCK", host_lock.as_os_str()),
            ("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "0".as_ref()),
        ],
        "exec 9>>\"$DREP_MUTANTS_HOST_LOCK\" && perl -MFcntl=:flock -e 'open(my $l, \">&=\", 9) or exit 2; flock($l, LOCK_EX) or exit 1' && ",
    );

    assert!(output.status.success(), "{output:?}");
    assert!(temp.path().join("cargo-args").exists());
}

/// Another sweep's host lock makes the run wait, here for no time at all, and
/// give up with 75 before it starts cargo-mutants.
#[cfg(unix)]
#[test]
fn a_run_waits_for_a_host_lock_another_sweep_holds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let host_lock = temp.path().join("host.lock");
    let mut other = common::hold_lock(&host_lock, "30");

    let output = run_with_fake_cargo(
        temp.path(),
        &temp.path().join("scratch"),
        &[
            ("DREP_MUTANTS_HOST_LOCK", host_lock.as_os_str()),
            ("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "0".as_ref()),
        ],
        "",
    );
    let _ = other.kill();
    let _ = other.wait();

    assert_eq!(output.status.code(), Some(75), "{output:?}");
    assert!(!temp.path().join("cargo-args").exists());
}
