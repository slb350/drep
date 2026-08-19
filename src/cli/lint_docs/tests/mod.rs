//! Tests for `drep lint-docs`.
//!
//! Every file here must be declared below - Rust silently ignores a test file
//! no `mod` points at.

mod gating;
mod render;
mod resolve;

use std::path::Path;

use tempfile::TempDir;

use crate::cli::lint_docs::{LintDocsArgs, LintOutcome, analyze};

/// A temp directory with the named files written into it.
///
/// Paths may contain a directory component; parents are created.
fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, content).expect("write");
    }
    dir
}

/// Run `lint-docs` over `root` with the given arguments.
///
/// `paths` are joined onto `root`, which is what a user gets by `cd`-ing into
/// the repository: in production `root` is the process cwd (`"."`), so an
/// argument and the walk root are relative to the same place. The parameter
/// exists so a test does not have to chdir a shared process.
fn run_in(root: &Path, paths: &[&str], strict: bool) -> LintOutcome {
    let args = LintDocsArgs {
        paths: paths.iter().map(|p| root.join(p)).collect(),
        strict,
    };
    analyze(&args, root)
}

/// Run over the whole of `root` (no explicit paths), report-only.
fn walk(root: &Path) -> LintOutcome {
    run_in(root, &[], false)
}
