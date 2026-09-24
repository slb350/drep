//! Helpers shared by the integration tests that assert over config files.
//!
//! Integration tests are separate crates, so this is the only sharing point
//! they have short of the library's public API - and the reader below has
//! already been transcribed once. `src/test_support.rs` is the wrong home: it
//! is `pub(crate)` and holds mock-endpoint fixtures.

/// A UTF-8 file from the repository root.
pub fn read(relative: &str) -> String {
    let path = format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} must be readable: {e}"))
}

/// A config file from the repository root, with its comment lines stripped.
///
/// The generated files carry an explanatory comment above every key, so a
/// raw-text assertion would be satisfied by the comment describing a setting
/// as readily as by the setting.
pub fn without_comments(relative: &str) -> String {
    read(relative)
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes an executable test stub from a child process, so no descriptor of
/// this process can hold it open for writing when another test thread forks:
/// Linux refuses to `exec` such a file (`Text file busy`). Mirrors
/// `test_support::write_executable`, which integration tests cannot reach.
// Only release_config uses the process helpers; published_hooks shares this module.
#[allow(dead_code)]
#[cfg(unix)]
pub fn write_executable(path: &std::path::Path, contents: &str) {
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"printf '%s' "$2" > "$1" && chmod +x "$1""#)
        .arg("sh")
        .arg(path)
        .arg(contents)
        .status()
        .expect("the writer process must start");
    assert!(
        status.success(),
        "writing the executable {} failed: {status}",
        path.display()
    );
}

/// A command whose git calls cannot reach the repository of a hook this suite
/// may be running under. Mirrors the environment `test_support::git` clears.
#[allow(dead_code)]
pub fn without_outer_git(program: &str, dir: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_QUARANTINE_PATH",
    ] {
        command.env_remove(variable);
    }
    command.current_dir(dir);
    command
}
