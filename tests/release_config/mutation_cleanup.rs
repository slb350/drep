use super::common;

/// Mutation scratch copies live beside the checkout, never in the system temp dir.
#[test]
fn mutation_scratch_copies_stay_off_the_tmpfs() {
    let script = common::without_comments("scripts/mutants-run.sh");

    assert!(
        script.contains("export TMPDIR=\"${DREP_MUTANTS_TMPDIR:-${ROOT}.mutants-tmp}\""),
        "the shared runner must place scratch copies beside the checkout"
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
        script.contains("-name 'cargo-mutants-*.tmp'")
            && script.contains("-path \"$TMPDIR\"/'cargo-mutants-*.tmp/*'")
            && script.contains("-name 'drep-diff-test-*'")
            && script.contains("-path \"$TMPDIR\"/'drep-diff-test-*/*'")
            && script.contains("-delete"),
        "cleanup must delete only known mutation and test scratch under TMPDIR"
    );
    assert!(
        script.contains("trap cleanup_mutation_scratch EXIT"),
        "the run must remove its own copies on exit"
    );
}

/// The cleanup expression is destructive only inside its two known prefixes.
#[cfg(unix)]
#[test]
fn mutation_scratch_cleanup_preserves_adjacent_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = temp.path().join("scratch");
    let stale = scratch.join("cargo-mutants-stale.tmp/nested");
    let stale_test = scratch.join("drep-diff-test-stale/nested");
    let adjacent = scratch.join("cargo-mutants-stale.tmp.keep");
    let outside = temp.path().join("outside");
    let captured_args = temp.path().join("cargo-args");
    std::fs::create_dir_all(&stale).expect("stale scratch tree");
    std::fs::write(stale.join("file"), "stale").expect("stale scratch file");
    std::fs::create_dir_all(&stale_test).expect("stale test scratch tree");
    std::fs::create_dir_all(&adjacent).expect("adjacent directory");
    std::fs::write(adjacent.join("keep"), "keep").expect("adjacent file");
    std::fs::create_dir_all(&outside).expect("outside directory");
    std::fs::write(outside.join("keep"), "keep").expect("outside file");
    std::os::unix::fs::symlink(&outside, scratch.join("cargo-mutants-link.tmp"))
        .expect("scratch symlink");

    let script = format!("{}/scripts/mutants-run.sh", env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("bash")
        .args([
            "-c",
            "cargo() { mkdir -p \"$TMPDIR/cargo-mutants-trap.tmp/nested\" \"$TMPDIR/drep-diff-test-trap/nested\"; printf '%s\\n' \"$@\" >\"$DREP_MUTANTS_CAPTURE_ARGS\"; }; export -f cargo; \"$1\"",
            "mutation-cleanup-test",
            &script,
        ])
        .env("DREP_MUTANTS_TMPDIR", &scratch)
        .env("DREP_MUTANTS_CAPTURE_ARGS", &captured_args)
        .env("MUTANTS_OUT_DIR", temp.path().join("out"))
        .env_remove("DREP_MUTANTS_HOST_LOCK")
        .env_remove("DREP_MUTANTS_HOST_LOCK_WAIT_SECONDS")
        .env_remove("DREP_MUTANTS_RESULT_TOKEN")
        .output()
        .expect("run mutation wrapper with fake cargo");

    assert!(
        output.status.success(),
        "runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!scratch.join("cargo-mutants-stale.tmp").exists());
    assert!(!scratch.join("cargo-mutants-trap.tmp").exists());
    assert!(!scratch.join("drep-diff-test-stale").exists());
    assert!(!scratch.join("drep-diff-test-trap").exists());
    assert!(!scratch.join("cargo-mutants-link.tmp").exists());
    assert!(adjacent.join("keep").exists());
    assert!(outside.join("keep").exists());

    let args = std::fs::read_to_string(captured_args).expect("captured cargo arguments");
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
