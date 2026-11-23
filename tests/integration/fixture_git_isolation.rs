//! Every test, inline test modules included, reaches git through `crate::test_support::git` or `tests/common::without_outer_git`, which keep the environment of a hook this suite may run under away from a fixture. A bare spawn inherits it and acts on the repository being committed instead of the fixture's own.

use std::fs;
use std::path::{Path, PathBuf};

/// The files that may spawn git directly: the shared definition and the unit tests' helper.
const HELPERS: [&str; 2] = ["src/test_git_env.rs", "src/test_support.rs"];

/// A bare spawn, spelled so this file does not match itself.
const BARE_GIT: &str = concat!("Command::new(", "\"git\")");

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("list sources") {
        let path = entry.expect("read a source entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// Test code: everything under `tests/`, and a `src` file inside a `tests` directory or named for tests.
fn is_test_code(relative: &Path) -> bool {
    relative.starts_with("tests")
        || relative
            .components()
            .any(|part| part.as_os_str() == "tests")
        || relative.file_stem().is_some_and(|stem| {
            let stem = stem.to_string_lossy();
            stem == "tests" || stem.ends_with("_tests") || stem.starts_with("test_")
        })
}

/// The lines of `text` that are test code: all of a test file, and an ordinary source file from its first `#[cfg(test)]` on, where its inline test modules sit.
fn test_lines<'a>(relative: &Path, text: &'a str) -> impl Iterator<Item = (usize, &'a str)> {
    let whole = is_test_code(relative);
    let mut in_tests = whole;
    text.lines().enumerate().filter(move |(_, line)| {
        in_tests = in_tests || line.contains("#[cfg(test)]");
        in_tests
    })
}

#[test]
fn tests_spawn_git_only_through_the_isolating_helpers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("tests"), &mut files);
    let mut offenders = Vec::new();
    for path in files {
        let relative = path.strip_prefix(root).expect("path under the crate");
        if HELPERS.iter().any(|helper| relative == Path::new(helper)) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read a source");
        for (number, line) in test_lines(relative, &text) {
            let code = line.split("//").next().unwrap_or_default();
            if code.contains(BARE_GIT) {
                offenders.push(format!("{}:{}", relative.display(), number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "spawn git through test_support::git or common::without_outer_git instead: {offenders:?}"
    );
}
