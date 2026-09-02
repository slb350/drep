//! End-to-end run_tool behaviour against real /bin/sh scripts.
//!
//! Wired in via `#[cfg(test)] mod tests;` in the parent module. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration, so
//! cargo never compiled them and appending invalid Rust did not fail the build.
//! If you add a file here, declare it in this directory's `mod.rs`.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;

// ---- run_tool ----

#[tokio::test]
async fn run_tool_returns_skipped_when_no_config_file_present() {
    let spec = ToolSpec {
        name: "ruff",
        local_paths: &["nope"],
        command: &["definitely-not-installed-ruff-xyz"],
        config_files: &["pyproject.toml"],
        output_format: "json",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    let dir = TempDir::new().unwrap();
    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Skipped);
}

#[tokio::test]
async fn run_tool_returns_unavailable_when_binary_cannot_be_found() {
    let spec = ToolSpec {
        name: "no-such-tool",
        local_paths: &[],
        command: &["definitely-not-installed-tool-zzz"],
        config_files: &["any"],
        output_format: "json",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("any"), "").unwrap();
    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Unavailable);
}

#[tokio::test]
async fn compilation_ground_truth_requires_a_successful_compiler() {
    for (establishes_compilation, expected) in [(true, true), (false, false)] {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("checker");
        write_executable(&bin, "#!/bin/sh\nexit 0\n");
        std::fs::write(dir.path().join("project.config"), "").unwrap();
        let spec = ToolSpec {
            name: "checker",
            command: &["checker"],
            local_paths: &["checker"],
            config_files: &["project.config"],
            output_format: "lines",
            establishes_compilation,
            ..ToolSpec::default()
        };

        let outcome = run_tool(&spec, dir.path(), &["src/lib.rs".to_owned()]).await;

        assert_eq!(outcome.status, ToolStatus::Ok);
        assert_eq!(
            outcome.compilation_succeeded, expected,
            "a successful linter cannot disprove an LLM compile-error claim"
        );
    }
}

#[tokio::test]
async fn run_tool_reads_stderr_when_diagnostics_stream_is_stderr() {
    // Build a tiny shell script in a temp dir; it writes parseable JSON
    // to stderr and nothing to stdout. With `diagnostics_stream = "stderr"`
    // we expect one finding; flipping the stream to stdout drops it
    // (stdout is empty, so it parses as `[]`).
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("diag");
    write_executable(
        &bin,
        "#!/bin/sh\nprintf '%s' '[{\"code\":\"E1\",\"filename\":\"x\",\"location\":{\"row\":1,\"column\":1},\"message\":\"m\"}]' 1>&2\n",
    );

    let spec = ToolSpec {
        name: "diag",
        command: &["diag"],
        local_paths: &["diag"],
        config_files: &["marker"],
        output_format: "json",
        diagnostics_stream: "stderr",
        ..ToolSpec::default()
    };
    std::fs::write(dir.path().join("marker"), "").unwrap();

    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Ok);
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(outcome.findings[0].kind, "E1");

    // Same tool with `diagnostics_stream = "stdout"` produces nothing
    // from stderr - the round-trip proves the stream is being honoured.
    let spec_stdout = ToolSpec {
        diagnostics_stream: "stdout",
        ..spec
    };
    let outcome = run_tool(&spec_stdout, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Ok);
    assert!(
        outcome.findings.is_empty(),
        "stdout-streamed run should not pick up stderr diagnostics"
    );
}

#[tokio::test]
async fn run_tool_reports_unavailable_for_unparseable_output_not_empty_ok() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("noisy");
    write_executable(&bin, "#!/bin/sh\necho 'this is not json'\n");

    let spec = ToolSpec {
        name: "noisy",
        command: &["noisy"],
        local_paths: &["noisy"],
        config_files: &["marker"],
        output_format: "json",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    std::fs::write(dir.path().join("marker"), "").unwrap();

    let outcome = run_tool(&spec, dir.path(), &[]).await;
    assert_eq!(outcome.status, ToolStatus::Unavailable);
    assert!(
        outcome.findings.is_empty(),
        "unparseable must not be reported as zero findings"
    );
}

/// A tool that exits non-zero having produced no diagnostics is `Unavailable`,
/// not a clean `Ok`.
///
/// The exit code alone is not a verdict - ruff and clippy exit non-zero
/// *because* they found issues - but a non-zero exit with nothing on the
/// diagnostics stream means the tool did not run: bad config, crash, bad
/// invocation. Reporting that as `Ok` with zero findings is the
/// "unavailable is not a pass" failure this module exists to prevent.
#[tokio::test]
async fn a_silent_non_zero_exit_is_unavailable_not_a_clean_pass() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("failtool");
    write_executable(&bin, "#!/bin/sh\necho 'fatal: bad config' >&2\nexit 2\n");
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();

    // `lines`, not `json`. An empty stdout is not valid JSON, so a `json`
    // spec reaches `Unavailable` through the *parse-failure* path and the test
    // passes without ever exercising the exit-status rule - which is exactly
    // what happened on the first draft. The `lines` parser accepts empty input
    // as zero findings, so only the new rule can produce `Unavailable` here.
    let spec = ToolSpec {
        name: "failtool",
        command: &["failtool"],
        local_paths: &["failtool"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, dir.path(), &["a.py".to_owned()]).await;
    assert_eq!(
        outcome.status,
        ToolStatus::Unavailable,
        "a silent non-zero exit must not read as a clean pass, got {outcome:?}"
    );
    assert!(
        outcome.detail.contains("fatal: bad config"),
        "the other stream carries the real error and must reach the detail: {}",
        outcome.detail
    );
}

/// A relative `root` still resolves the repo-local tool.
///
/// `resolve_tool` returns `root.join(relative)`, and the child is spawned with
/// `current_dir(root)` - so a relative root made the child resolve the
/// executable a second time, from inside root: `repo/repo/node_modules/...`.
/// It worked only because the CLI passes "." and the tests pass absolute temp
/// dirs.
#[tokio::test]
async fn a_relative_root_still_finds_the_repo_local_tool() {
    let dir = TempDir::new().unwrap();
    let cwd = std::env::current_dir().expect("cwd");
    let relative = pathdiff_relative(&cwd, dir.path());

    let bin = dir.path().join("mytool");
    write_executable(&bin, "#!/bin/sh\nexit 0\n");
    std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();

    let spec = ToolSpec {
        name: "mytool",
        command: &["mytool"],
        local_paths: &["mytool"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, &relative, &[]).await;
    assert_ne!(
        outcome.status,
        ToolStatus::Unavailable,
        "a relative root must still spawn the repo-local tool, got {outcome:?}"
    );
}

/// A path for `to` expressed relative to `from`, when `to` is absolute and
/// shares no prefix worth trimming: falls back to the absolute path.
fn pathdiff_relative(from: &std::path::Path, to: &std::path::Path) -> std::path::PathBuf {
    to.strip_prefix(from)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|_| to.to_path_buf())
}

/// A tool declaring `accepts_files: false` is invoked with **no** file
/// arguments.
///
/// `cargo clippy` checks a crate and rejects a path argument outright with
/// "unexpected argument", so appending files made every Rust run fail. drep
/// reported that honestly as `Unavailable` rather than as a clean file - which
/// is why it surfaced as exit 2 on every Rust repository instead of as silence
/// - but the effect was that the deterministic half for Rust never ran at all.
/// The stub records its argv so the absence is asserted directly.
#[tokio::test]
async fn a_whole_project_tool_is_invoked_without_file_arguments() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("marker"), "").expect("config file");

    let log = root.join("argv.log");
    let stub = format!(
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> {}; done\nprintf '[]\\n'\n",
        log.to_string_lossy()
    );
    let tool = root.join("wholeproject");
    write_executable(&tool, stub);

    let spec = ToolSpec {
        name: "wholeproject",
        command: &["wholeproject", "--flag"],
        local_paths: &["wholeproject"],
        config_files: &["marker"],
        output_format: "lines",
        accepts_files: false,
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, root, &["a.rs".to_owned(), "b.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);

    let recorded: Vec<String> = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        recorded,
        vec!["--flag"],
        "the declared flags are passed and the files are not"
    );
}

/// A whole-project tool's findings are narrowed to the files being checked.
///
/// It reports on the entire crate, so without the filter a commit gate would
/// block on pre-existing issues in code the commit never touched - unfixable
/// by the author, and every commit would fail until the whole crate was clean.
/// The `./` prefix on one requested path is deliberate: the dash-guard adds it,
/// and the tool reports paths without it.
#[tokio::test]
async fn a_whole_project_tools_findings_are_narrowed_to_the_requested_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("marker"), "").expect("config file");

    // `lines` format: each line names a file the tool has an opinion about.
    let stub = "#!/bin/sh\nprintf 'wanted.rs\\nuntouched.rs\\nalso_wanted.rs\\n'\n";
    let tool = root.join("wholeproject");
    write_executable(&tool, stub);

    let spec = ToolSpec {
        name: "wholeproject",
        command: &["wholeproject"],
        local_paths: &["wholeproject"],
        config_files: &["marker"],
        output_format: "lines",
        accepts_files: false,
        ..ToolSpec::default()
    };

    let outcome = run_tool(
        &spec,
        root,
        &["wanted.rs".to_owned(), "./also_wanted.rs".to_owned()],
    )
    .await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);

    let mut reported: Vec<&str> = outcome
        .findings
        .iter()
        .map(|f| f.file_path.as_str())
        .collect();
    reported.sort_unstable();
    assert_eq!(
        reported,
        vec!["also_wanted.rs", "wanted.rs"],
        "untouched.rs was not asked about, so its finding must be dropped"
    );
}

/// A tool that *does* accept files keeps every finding it reports.
///
/// The other half of the filter: applying it unconditionally would silently
/// drop findings whenever a tool reported a path in a different but equivalent
/// form, so the narrowing must be scoped to the tools that need it.
#[tokio::test]
async fn a_file_taking_tools_findings_are_not_filtered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("marker"), "").expect("config file");

    let stub = "#!/bin/sh\nprintf 'somewhere/else.rs\\n'\n";
    let tool = root.join("perfile");
    write_executable(&tool, stub);

    let spec = ToolSpec {
        name: "perfile",
        command: &["perfile"],
        local_paths: &["perfile"],
        config_files: &["marker"],
        output_format: "lines",
        ..ToolSpec::default()
    };

    let outcome = run_tool(&spec, root, &["asked.rs".to_owned()]).await;
    assert_eq!(outcome.status, ToolStatus::Ok, "detail: {}", outcome.detail);
    assert_eq!(
        outcome.findings.len(),
        1,
        "a per-file tool only reports on what it was given, so nothing is dropped"
    );
}

/// A file whose name begins with `-` is passed as a path, not an option.
///
/// A repository can legitimately contain `--fix`, and every checker drep runs
/// would read that as a flag. The guard is a `./` prefix rather than a `--`
/// separator, because `--` is not universally accepted across
/// ruff/eslint/tsc/gofmt/go vet/clippy while `./` is unambiguous to any
/// argument parser.
#[tokio::test]
async fn a_filename_that_looks_like_a_flag_is_passed_as_a_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir bin");
    let tool = bin.join("argvdump");
    write_executable(
        &tool,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}/argv.txt\nexit 0\n",
            dir.path().display()
        ),
    );
    std::fs::write(dir.path().join("pyproject.toml"), "").expect("config");

    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump"],
        local_paths: &["bin/argvdump"],
        config_files: &["pyproject.toml"],
        output_format: "lines",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };

    let _ = run_tool(&spec, dir.path(), &["--fix".to_owned()]).await;

    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap_or_default();
    assert!(
        argv.lines().any(|a| a == "./--fix"),
        "a dash-leading filename must reach the tool as `./--fix`, got: {argv:?}"
    );
    assert!(
        !argv.lines().any(|a| a == "--fix"),
        "it must not reach the tool as a bare option: {argv:?}"
    );
}

// ---- run_tool: config_flag ----

/// Builds an argv-recording stub in a temp repo, runs `spec` against it, and
/// returns what the tool actually received.
fn argv_after_running(spec: &ToolSpec, config_files: &[&str]) -> (TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir bin");
    write_executable(
        &bin.join("argvdump"),
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}/argv.txt\nexit 0\n",
            dir.path().display()
        ),
    );
    for config in config_files {
        let path = dir.path().join(config);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir config parent");
        }
        std::fs::write(path, "").expect("config");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(run_tool(spec, dir.path(), &["Sample.java".to_owned()]));
    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap_or_default();
    let lines = argv.lines().map(str::to_owned).collect();
    (dir, lines)
}

/// checkstyle exits 1 with "Must specify a config XML" when run bare, so the
/// ruleset `config_files` found has to reach it or the tool can never run.
#[test]
fn config_flag_passes_the_discovered_config_before_the_files() {
    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump", "-f", "sarif"],
        local_paths: &["bin/argvdump"],
        config_files: &["config/checkstyle/checkstyle.xml"],
        config_flag: Some("-c"),
        output_format: "sarif",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    let (_dir, argv) = argv_after_running(&spec, &["config/checkstyle/checkstyle.xml"]);
    assert_eq!(
        argv,
        vec![
            "-f",
            "sarif",
            "-c",
            "config/checkstyle/checkstyle.xml",
            "Sample.java"
        ],
        "the config must land between the static command and the files"
    );
}

/// The first entry in `config_files` that exists wins, matching the order the
/// list is written in rather than whatever the filesystem returns.
#[test]
fn config_flag_uses_the_first_config_file_that_exists() {
    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump"],
        local_paths: &["bin/argvdump"],
        config_files: &["checkstyle.xml", "config/checkstyle/checkstyle.xml"],
        config_flag: Some("-c"),
        output_format: "sarif",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    let (_dir, argv) = argv_after_running(&spec, &["config/checkstyle/checkstyle.xml"]);
    assert_eq!(
        argv,
        vec!["-c", "config/checkstyle/checkstyle.xml", "Sample.java"]
    );
}

/// A tool that finds its own config is untouched, which is every tool that
/// shipped before this one.
#[test]
fn no_config_flag_leaves_the_command_alone() {
    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump", "check"],
        local_paths: &["bin/argvdump"],
        config_files: &["pyproject.toml"],
        config_flag: None,
        output_format: "lines",
        diagnostics_stream: "stdout",
        ..ToolSpec::default()
    };
    let (_dir, argv) = argv_after_running(&spec, &["pyproject.toml"]);
    assert_eq!(argv, vec!["check", "Sample.java"]);
}
