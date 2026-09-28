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
use crate::test_support::write_executable;

/// Build a `Work` with one whole-file hunk per path. Empty
/// `read_failures` because deterministic tests start from a clean
/// input-resolution step.
fn work_for(paths: &[PathBuf]) -> Work {
    Work {
        lint_only: Vec::new(),
        reviewed_directories: std::collections::BTreeSet::new(),
        by_file: paths
            .iter()
            .map(|p| vec![Hunk::whole_file(p.clone(), "x = 1\n")])
            .collect(),
        read_failures: BTreeMap::new(),
        uncommitted: std::collections::BTreeSet::new(),
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
    write_executable(
        &bin,
        format!("#!/bin/sh\nprintf '%s\\n' called >> {counter_str}\nprintf '%s' '[]'\n"),
    );

    // A `pyproject.toml` makes the project opted into ruff; otherwise the
    // tool would be `Skipped` and the counter would stay at zero for an
    // unrelated reason.
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");

    let a = dir.path().join("a.py");
    let b = dir.path().join("b.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    std::fs::write(&b, "b = 2\n").expect("b.py");

    let work = work_for(&[a.clone(), b.clone()]);
    let (_findings, _failures, _compiled) = deterministic::run(&work, dir.path()).await;

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
    write_executable(&bin, "#!/bin/sh\nprintf '%s' 'this is not json, sorry'\n");
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("pyproject");

    let a = dir.path().join("a.py");
    let b = dir.path().join("b.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");
    std::fs::write(&b, "b = 2\n").expect("b.py");

    let work = work_for(&[a.clone(), b.clone()]);
    let (findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

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
    let (findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        findings.is_empty(),
        "Skipped must contribute no findings, got {findings:?}"
    );
    assert!(
        failures.is_empty(),
        "Skipped must contribute no failures, got {failures:?}"
    );
}

/// A monorepo tool is run from the nearest configured workspace, while a
/// dependency hoisted to the repository root remains resolvable.
#[tokio::test]
async fn nested_workspace_config_runs_with_workspace_relative_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let argv = root.join("argv.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > {}\nprintf '%s' '[]'\n",
            argv.display()
        ),
    );

    let member = root.join("apps/web");
    std::fs::create_dir_all(member.join("src")).expect("member dirs");
    std::fs::write(member.join("pyproject.toml"), "").expect("member config");
    let file = member.join("src/app.py");
    std::fs::write(&file, "x = 1\n").expect("source");

    let work = work_for(std::slice::from_ref(&file));
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;

    assert!(failures.is_empty(), "workspace tool failed: {failures:?}");
    let recorded = std::fs::read_to_string(argv).expect("recorded argv");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(
        std::path::Path::new(lines[0])
            .canonicalize()
            .expect("actual cwd"),
        member.canonicalize().expect("member cwd")
    );
    assert_eq!(lines.last().copied(), Some("src/app.py"));
}

/// Files under different configured members are separate tool batches.
#[tokio::test]
async fn separate_workspace_configs_produce_separate_invocations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = root.join("counter.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" >> {}\nprintf '%s' '[]'\n",
            counter.display()
        ),
    );

    let files: Vec<PathBuf> = ["apps/web", "packages/core"]
        .into_iter()
        .map(|member| {
            let member = root.join(member);
            std::fs::create_dir_all(&member).expect("member dir");
            std::fs::write(member.join("pyproject.toml"), "").expect("member config");
            let file = member.join("mod.py");
            std::fs::write(&file, "x = 1\n").expect("source");
            file
        })
        .collect();

    let work = work_for(&files);
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(failures.is_empty(), "workspace tools failed: {failures:?}");
    assert_eq!(
        std::fs::read_to_string(counter)
            .expect("counter")
            .lines()
            .count(),
        2
    );
}

/// A `ruff` fixture: the stub appends one line (its argv, joined) to
/// `argv.txt` per invocation, so a refusal is observable as the file never
/// appearing and a run as exactly one line.
///
/// Returns the stub's argv record. `pyproject.toml` opts the project into
/// ruff; without it the tool is `Skipped` and a refusal test would pass for
/// the wrong reason.
fn ruff_fixture(root: &std::path::Path) -> PathBuf {
    let bin = root.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let argv = root.join("argv.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s' '[]'\n",
            argv.display()
        ),
    );
    std::fs::write(root.join("pyproject.toml"), "").expect("pyproject");
    argv
}

/// Staged mode, criterion: a per-file tool whose file differs from the
/// committing index is not run - it would lint content the commit does not
/// hold - and the file fails with `UncommittedChanges` naming it.
#[tokio::test]
async fn a_tool_whose_file_is_uncommitted_does_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("a.py")].into_iter().collect();
    let (findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run on content the commit does not hold"
    );
    assert!(
        findings.is_empty(),
        "a refused tool contributes no findings, got {findings:?}"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("a.py")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A per-file tool also reads the config file its workspace holds, so an
/// uncommitted *config* refuses the run even when the linted file is clean.
#[tokio::test]
async fn a_tool_whose_config_file_is_uncommitted_does_not_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("pyproject.toml")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        !argv.exists(),
        "ruff must not run against a config the commit does not hold"
    );
    match failures.get(&a) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "ruff");
            assert_eq!(paths, &[PathBuf::from("pyproject.toml")]);
        }
        other => panic!("expected UncommittedChanges for {a:?}, got {other:?}"),
    }
}

/// A clean file's tool runs exactly as before when the only uncommitted path
/// is an unrelated one the task never reads.
#[tokio::test]
async fn a_clean_files_tool_runs_when_an_unrelated_path_is_uncommitted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = ruff_fixture(dir.path());
    let a = dir.path().join("a.py");
    std::fs::write(&a, "a = 1\n").expect("a.py");

    let mut work = work_for(std::slice::from_ref(&a));
    work.uncommitted = [PathBuf::from("unrelated.py")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, dir.path()).await;

    assert!(
        failures.is_empty(),
        "an unrelated difference must not refuse the tool, got {failures:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&argv)
            .expect("recorded argv")
            .lines()
            .count(),
        1,
        "the tool must run exactly once"
    );
}

/// A whole-project tool (`accepts_files: false`) is invoked bare from its
/// workspace and reads all of it, so any differing path under the workspace
/// refuses the run - and one outside it does not.
#[tokio::test]
async fn a_whole_project_tool_refuses_any_uncommitted_path_under_its_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let bin = root.join("node_modules/.bin/tsc");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    let counter = root.join("counter.txt");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' called >> {}\n",
            counter.display()
        ),
    );
    let member = root.join("apps/web");
    std::fs::create_dir_all(member.join("src")).expect("member dirs");
    std::fs::write(member.join("tsconfig.json"), "{}\n").expect("tsconfig");
    let file = member.join("src/app.ts");
    std::fs::write(&file, "const x: number = 1;\n").expect("source");

    let mut work = work_for(std::slice::from_ref(&file));
    work.uncommitted = [PathBuf::from("apps/web/tsconfig.json")]
        .into_iter()
        .collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(
        !counter.exists(),
        "tsc must not run while its workspace differs from the commit"
    );
    match failures.get(&file) {
        Some(FailureReason::UncommittedChanges { tool, paths }) => {
            assert_eq!(tool, "tsc");
            assert_eq!(paths, &[PathBuf::from("apps/web/tsconfig.json")]);
        }
        other => panic!("expected UncommittedChanges for {file:?}, got {other:?}"),
    }

    let mut work = work_for(std::slice::from_ref(&file));
    work.uncommitted = [PathBuf::from("other.py")].into_iter().collect();
    let (_findings, failures, _compiled) = deterministic::run(&work, root).await;
    assert!(
        failures.is_empty(),
        "a difference outside the workspace must not refuse the tool, got {failures:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&counter)
            .expect("counter")
            .lines()
            .count(),
        1,
        "the tool must run once its workspace no longer differs"
    );
}

/// A finding reported through a resolved symlink is rewritten back to the
/// path the user asked about, not left in the tool's spelling.
///
/// `run_one` maps a whole-project tool's absolute reported paths back to the
/// caller's originals byte-exact, but the tool derives its paths from a cwd
/// the OS resolved - on a symlinked checkout its spelling and drep's differ,
/// the rewrite missed, and the finding kept a path that names a file the
/// user never typed (and that acknowledgement fingerprints, cache keys and
/// the report all then disagree about).
#[cfg(unix)]
#[tokio::test]
async fn findings_rewrite_through_a_symlinked_checkout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("real");
    std::fs::create_dir(&real).expect("real dir");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let bin = real.join("venv/bin/ruff");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("bin dir");
    std::fs::write(real.join("asked.py"), "x = 1\n").expect("source");
    // The stub answers with the canonical spelling of the file, as a tool
    // deriving paths from its own resolved cwd does.
    let canonical = real.join("asked.py").canonicalize().expect("canonical");
    write_executable(
        &bin,
        format!(
            "#!/bin/sh\nprintf '%s' '[{{\"code\":\"E1\",\"filename\":\"{}\",\"location\":{{\"row\":1,\"column\":1}},\"message\":\"m\"}}]'\n",
            canonical.to_string_lossy()
        ),
    );
    std::fs::write(real.join("pyproject.toml"), "").expect("pyproject");

    let asked = link.join("asked.py");
    let work = work_for(std::slice::from_ref(&asked));
    let (findings, failures, _compiled) = deterministic::run(&work, &link).await;

    assert!(failures.is_empty(), "tool failed: {failures:?}");
    assert_eq!(findings.len(), 1, "the finding must survive the rewrite");
    assert_eq!(
        findings[0].file_path,
        asked.to_string_lossy(),
        "the finding must come back in the spelling the user asked about"
    );
}
