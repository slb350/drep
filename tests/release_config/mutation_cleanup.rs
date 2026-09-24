use super::common;

/// Runs `mutants-run.sh` against `scratch` with a cargo that records its
/// arguments and TMPDIR and plants copies the run must clean up.
#[cfg(unix)]
fn run_with_fake_cargo(
    temp: &std::path::Path,
    scratch: &std::path::Path,
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let script = format!("{}/scripts/mutants-run.sh", env!("CARGO_MANIFEST_DIR"));
    let mut command = std::process::Command::new("bash");
    command
        .args([
            "-c",
            "cargo() { mkdir -p \"$TMPDIR/cargo-mutants-trap.tmp/nested\" \"$TMPDIR/.tmp-test-debris\"; printf '%s\\n' \"$@\" >\"$DREP_MUTANTS_CAPTURE_ARGS\"; printf '%s\\n' \"$TMPDIR\" >\"$DREP_MUTANTS_CAPTURE_TMPDIR\"; }; export -f cargo; \"$1\"",
            "mutation-cleanup-test",
            &script,
        ])
        .env("DREP_MUTANTS_TMPDIR", scratch)
        .env("DREP_MUTANTS_CAPTURE_ARGS", temp.join("cargo-args"))
        .env("DREP_MUTANTS_CAPTURE_TMPDIR", temp.join("cargo-tmpdir"))
        .env("MUTANTS_OUT_DIR", temp.join("out"))
        .env_remove("DREP_MUTANTS_HOST_LOCK")
        .env_remove("DREP_MUTANTS_RESULT_TOKEN")
        .env_remove("MUTANTS_CHECKOUT_LOCK_HELD");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .output()
        .expect("run mutation wrapper with fake cargo")
}

/// Whether another process could take the checkout lock right now.
#[cfg(unix)]
fn checkout_lock_is_free(lock: &std::path::Path) -> bool {
    std::process::Command::new("perl")
        .args([
            "-MFcntl=:flock",
            "-e",
            "open(my $f, '>>', $ARGV[0]) or exit 2; exit(flock($f, LOCK_EX | LOCK_NB) ? 0 : 1)",
        ])
        .arg(lock)
        .status()
        .expect("probe the checkout lock")
        .success()
}

/// Mutation scratch copies live beside the checkout, never in the system temp dir.
#[test]
fn mutation_scratch_copies_stay_off_the_tmpfs() {
    let script = common::without_comments("scripts/mutants-run.sh");

    assert!(
        script.contains("SCRATCH_ROOT=\"${DREP_MUTANTS_TMPDIR:-${ROOT}.mutants-tmp}\"")
            && script.contains("RUN_SCRATCH=\"$SCRATCH_ROOT/run\"")
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
        script.contains("trap finish_run EXIT"),
        "the run must remove its own copies on exit"
    );
}

/// The sweep is destructive only inside the run directory and the two legacy
/// names, and the run removes its own directory, test debris included.
#[cfg(unix)]
#[test]
fn mutation_scratch_cleanup_preserves_adjacent_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let stale = scratch.join("cargo-mutants-stale.tmp/nested");
    let stale_test = scratch.join("drep-diff-test-stale/nested");
    let stale_run = scratch.join("run/.tmp-killed-test");
    let adjacent = scratch.join("cargo-mutants-stale.tmp.keep");
    let outside = temp.path().join("outside");
    for directory in [&stale, &stale_test, &stale_run, &adjacent, &outside] {
        std::fs::create_dir_all(directory).expect("seed directory");
    }
    std::fs::write(adjacent.join("keep"), "keep").expect("adjacent file");
    std::fs::write(outside.join("keep"), "keep").expect("outside file");
    std::os::unix::fs::symlink(&outside, scratch.join("cargo-mutants-link.tmp"))
        .expect("scratch symlink");

    let output = run_with_fake_cargo(temp.path(), &scratch, &[]);

    assert!(
        output.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!scratch.join("cargo-mutants-stale.tmp").exists());
    assert!(!scratch.join("drep-diff-test-stale").exists());
    assert!(!scratch.join("cargo-mutants-link.tmp").exists());
    assert!(
        !scratch.join("run").exists(),
        "the run must remove its directory, test debris included"
    );
    assert!(adjacent.join("keep").exists());
    assert!(outside.join("keep").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("cargo-tmpdir"))
            .expect("captured TMPDIR")
            .trim_end(),
        scratch.join("run").to_str().expect("utf-8 path"),
        "cargo must see the run directory as TMPDIR"
    );
    assert!(
        checkout_lock_is_free(&temp.path().join("out.lock")),
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

/// A second run in the same checkout waits for the first rather than deleting
/// its results or copies, and gives up with 75 instead of running.
#[cfg(unix)]
#[test]
fn a_second_run_in_one_checkout_waits_for_the_first() {
    use std::io::BufRead;
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let lock = temp.path().join("out.lock");
    let mut first = std::process::Command::new("perl")
        .args([
            "-MFcntl=:flock",
            "-e",
            "open(my $f, '>>', $ARGV[0]) or die; flock($f, LOCK_EX) or die; $| = 1; print \"locked\\n\"; sleep 30",
        ])
        .arg(&lock)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the first run's stand-in");
    let mut ready = String::new();
    std::io::BufReader::new(first.stdout.take().expect("stand-in stdout"))
        .read_line(&mut ready)
        .expect("stand-in reports the lock");
    assert_eq!(ready, "locked\n");
    let results = temp.path().join("out/mutants.out");
    std::fs::create_dir_all(&results).expect("first run's results");
    std::fs::write(results.join("missed.txt"), "first run's survivor\n")
        .expect("first run's verdict");

    let output = run_with_fake_cargo(
        temp.path(),
        &scratch,
        &[("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS", "1")],
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

/// The kernel drops the lock with its holder, so a lock file a killed run left
/// behind holds nothing.
#[cfg(unix)]
#[test]
fn a_leftover_lock_file_holds_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let lock = temp.path().join("out.lock");
    std::fs::write(&lock, "").expect("leftover lock file");

    let output = run_with_fake_cargo(temp.path(), &scratch, &[]);

    assert!(output.status.success(), "{output:?}");
    assert!(
        temp.path().join("cargo-args").exists(),
        "the run must go ahead"
    );
    assert!(
        checkout_lock_is_free(&lock),
        "the run must release the lock"
    );
}
