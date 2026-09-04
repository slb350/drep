//! End-to-end `run_tool` invocation behaviour against real /bin/sh scripts.

use tempfile::TempDir;

use super::support::*;
use crate::languages::runner::*;

#[tokio::test]
async fn run_tool_returns_skipped_when_no_config_file_present() {
    let spec = ToolSpec {
        name: "ruff",
        local_paths: &["nope"],
        command: &["definitely-not-installed-ruff-xyz"],
        config_files: &["pyproject.toml"],
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
            output_format: OutputFormat::Lines,
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
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stderr,
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
        diagnostics_stream: DiagnosticsStream::Stdout,
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
        output_format: OutputFormat::Json,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
        output_format: OutputFormat::Lines,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
    let log = root.join("argv.log");
    install_stub(
        root,
        "wholeproject",
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> {}; done\nprintf '[]\\n'\n",
            log.to_string_lossy()
        ),
    );

    // The extra argument is the point: a whole-project tool still gets its
    // own flags, and only the file list is withheld.
    let spec = ToolSpec {
        command: &["wholeproject", "--flag"],
        ..whole_project_lines_spec()
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
        output_format: OutputFormat::Lines,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
        output_format: OutputFormat::Sarif,
        diagnostics_stream: DiagnosticsStream::Stdout,
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

/// A glob marker paired with `config_flag` passes the file the glob matched,
/// not the glob.
///
/// Eligibility and the flag now ask one function which marker is present, so
/// they cannot disagree. They used to ask separately, and only the flag side
/// was ignorant of the leading `*.` form: such a tool was judged configured
/// by the glob and then run without the config it cannot start without.
/// Nothing shipped paired the two, so the hole was latent - `dotnet format`
/// has the glob markers and checkstyle has the flag.
#[test]
fn config_flag_passes_the_file_a_glob_marker_matched() {
    let spec = ToolSpec {
        name: "argvdump",
        command: &["argvdump"],
        local_paths: &["bin/argvdump"],
        config_files: &["*.xml"],
        config_flag: Some("-c"),
        output_format: OutputFormat::Sarif,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    };
    let (_dir, argv) = argv_after_running(&spec, &["rules.xml"]);
    assert_eq!(
        argv,
        vec!["-c", "rules.xml", "Sample.java"],
        "the glob must resolve to the matched file name before it reaches the tool"
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
        output_format: OutputFormat::Sarif,
        diagnostics_stream: DiagnosticsStream::Stdout,
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
        output_format: OutputFormat::Lines,
        diagnostics_stream: DiagnosticsStream::Stdout,
        ..ToolSpec::default()
    };
    let (_dir, argv) = argv_after_running(&spec, &["pyproject.toml"]);
    assert_eq!(argv, vec!["check", "Sample.java"]);
}
