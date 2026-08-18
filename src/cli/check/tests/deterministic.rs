//! The deterministic layer: criteria 9, 10, 11.
//!
//! Three states, three pins:
//!
//! - **Batching**: two files of one language yield one tool invocation,
//!   not two. A project with twenty Python files must pay one `ruff`
//!   start, not twenty.
//! - **`Unavailable` is a per-file failure**: a tool that runs (or refuses
//!   to run) for a batch must put every file in that batch into
//!   `failures`. The per-tool/per-file join is what the exit-2 contract
//!   rests on.
//! - **`Skipped` is not a failure**: a project with no `pyproject.toml`
//!   has not asked for ruff's opinion, and ruff is not on PATH either.
//!   Reporting this as `ToolUnavailable` would make every unconfigured
//!   project fail the gate.
//!
//! The three tests share a fake-binary pattern: write a shell script into
//! `venv/bin/ruff`, mark it executable, and either have it append a line
//! to a counter file (criterion 9) or output unparseable JSON (criterion
//! 10). The project is "configured" by writing a `pyproject.toml` next to
//! the files. Criterion 11 has neither script nor config, so the tool
//! stays `Skipped`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::analysis::result::FailureReason;
use crate::cli::check::deterministic;
use crate::cli::check::input::Work;
use crate::diff::hunks::Hunk;

/// Build a `Work` with one whole-file hunk per path. Empty
/// `read_failures` because deterministic tests start from a clean
/// input-resolution step.
fn work_for(paths: &[PathBuf]) -> Work {
    Work {
        by_file: paths
            .iter()
            .map(|p| vec![Hunk::whole_file(p.clone(), "x = 1\n")])
            .collect(),
        read_failures: BTreeMap::new(),
    }
}

/// Criterion 9: two Python files produce one tool invocation, not two.
///
/// The fake `ruff` appends one line to a counter file each time it runs.
/// After `deterministic::run` over a batch of two files, the counter file
/// must contain exactly one line. A regression that re-invoked the tool
/// per file would land two lines here; a regression that dropped the
/// batch entirely would land zero.
#[tokio::test]
async fn two_python_files_invoke_their_tool_once_not_twice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = dir.path().join("counter.txt");
    let counter_str = counter.to_string_lossy().into_owned();
    // Append one line per invocation. `printf` is more portable than `echo`
    // (echo's behaviour across shells diverges on `-n`, escapes, ...).
    std::fs::write(
        &bin,
        format!("#!/bin/sh\nprintf '%s\\n' called >> {counter_str}\nprintf '%s' '[]'\n"),
    )
    .expect("write ruff");
    make_executable(&bin);

    // A `pyproject.toml` makes the project opted into ruff; otherwise the
    // tool would be `Skipped` and the counter would stay at zero for an
    // unrelated reason.
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");

    let a = dir.path().join("a.py");
    let b = dir.path().join("b.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    std::fs::write(&b, "b = 2\n").expect("b.py");

    let work = work_for(&[a.clone(), b.clone()]);
    let (_findings, _failures) = deterministic::run(&work, dir.path()).await;

    let lines = std::fs::read_to_string(&counter)
        .map(|s| s.lines().count())
        .unwrap_or(0);
    assert_eq!(
        lines, 1,
        "two Python files must produce exactly one tool invocation; counter has {lines} line(s)"
    );
}

/// Criterion 10: a tool that yields `ToolStatus::Unavailable` puts
/// **every** file of its batch into `failures` with `ToolUnavailable`.
///
/// We force `Unavailable` by emitting unparseable JSON from the fake
/// binary: the tool runs (so the file list reaches the merge step), but
/// the parser fails and the outcome is `Unavailable`. The test uses two
/// files and asserts both paths are present.
#[tokio::test]
async fn unavailable_tool_marks_every_file_in_the_batch_as_failed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    // Output JSON the parser rejects. The exit code is irrelevant - ruff
    // exits non-zero on findings anyway - and the parse path treats any
    // unparseable payload as `Unavailable` regardless.
    std::fs::write(&bin, "#!/bin/sh\nprintf '%s' 'this is not json, sorry'\n").expect("write ruff");
    make_executable(&bin);
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");

    let a = dir.path().join("a.py");
    let b = dir.path().join("b.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    std::fs::write(&b, "b = 2\n").expect("b.py");

    let work = work_for(&[a.clone(), b.clone()]);
    let (findings, failures) = deterministic::run(&work, dir.path()).await;

    assert!(
        findings.is_empty(),
        "an Unavailable tool must contribute no findings, got {findings:?}"
    );
    for path in [&a, &b] {
        let reason = failures
            .get(path)
            .unwrap_or_else(|| panic!("missing failure for {path:?}, got {failures:?}"));
        assert!(
            matches!(reason, FailureReason::ToolUnavailable { .. }),
            "expected ToolUnavailable for {path:?}, got {reason:?}"
        );
    }
}

/// Criterion 11: a tool whose project is not configured for it is
/// `Skipped` and contributes **no** failure.
///
/// No `pyproject.toml`, no local `venv/bin/ruff`, and `ruff` is whatever
/// the system PATH happens to carry - any of those alone would force
/// `Unavailable` rather than `Skipped`, and the test would fail. The
/// fixture is the smallest one that exercises the `Skipped` branch:
/// configuration files absent, period.
#[tokio::test]
async fn unconfigured_tool_is_skipped_and_adds_no_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Deliberately no `pyproject.toml`, no `venv/bin/ruff`.
    let a = dir.path().join("a.py");
    let b = dir.path().join("b.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    std::fs::write(&b, "b = 2\n").expect("b.py");

    let work = work_for(&[a.clone(), b.clone()]);
    let (findings, failures) = deterministic::run(&work, dir.path()).await;

    assert!(
        findings.is_empty(),
        "Skipped must contribute no findings, got {findings:?}"
    );
    assert!(
        failures.is_empty(),
        "Skipped must contribute no failures, got {failures:?}"
    );
}

/// Mark `path` executable on Unix; no-op elsewhere.
#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}
