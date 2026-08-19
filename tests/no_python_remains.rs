//! 2.0 is a Rust binary, checked against a tree that spent a year being a
//! Python package.
//!
//! Deleting `drep/` is the easy half. The half that rots quietly is everything
//! that *pointed* at it: a CI job installing a package that no longer exists, a
//! commit hook calling a `./venv` nobody creates any more, a README telling
//! people to `pip install` something that stopped being published. Each of
//! those fails somewhere other than here - on a fresh clone, in a contributor's
//! first commit, on somebody else's machine - so they are asserted here.

mod common;

fn repo_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The Python package and the files that built it.
///
/// Directories, and files that are not `.py` - which is exactly what the walk
/// below cannot see. `tests/` survives, holding the Rust integration tests, so
/// it is the one directory that had to be emptied rather than removed.
#[test]
fn the_python_package_and_its_build_files_are_gone() {
    for relative in [
        "drep",
        "tests/unit",
        "tests/integration",
        "docs/api",
        "pyproject.toml",
        "uv.lock",
        "scripts/install.sh",
    ] {
        assert!(
            !repo_path(relative).exists(),
            "{relative} is 1.x and should have been deleted with the package"
        );
    }
}

/// No Python source anywhere in the tree drep still owns.
///
/// The list above names what was there on the day; a `.py` file arriving later
/// is the same mistake with a different path.
///
/// Pruning goes through `files::is_ignored_dir`, the same predicate the real
/// walk uses, so this cannot drift from drep's own answer to "which trees does
/// this project not own" - a second hand-written list would already have been
/// missing `node_modules`, `build` and `dist`. `repos/` and `external/` are
/// added on top: they are clones of other projects fetched for manual testing
/// (129 `.py` files between them), which belong to this machine rather than to
/// the repository.
///
/// Gitignore is deliberately *off*. A stray `.py` file that something later
/// gitignores is still a Python file in the tree, and the walk that respects
/// project policy is the wrong instrument for asking whether one is there.
#[test]
fn no_python_source_survives() {
    let root = repo_path(".");
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .require_git(false)
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                return !(drep::files::is_ignored_dir(&name)
                    || matches!(name.as_str(), "repos" | "external"));
            }
            true
        })
        .build();

    let found: Vec<_> = walker
        .flatten()
        .map(ignore::DirEntry::into_path)
        .filter(|path| path.extension().is_some_and(|ext| ext == "py"))
        .collect();
    assert!(found.is_empty(), "Python source is back: {found:?}");
}

/// The commit gate is Rust tooling and drep itself.
///
/// Every 1.x hook ran out of `./venv`, which Phase 8 stops creating. A hook
/// pointing at a missing interpreter fails the commit with a path error rather
/// than a finding, so it reads as the gate being broken.
#[test]
fn the_commit_gate_runs_no_python_tooling() {
    let config = common::without_comments(".pre-commit-config.yaml");
    for needle in ["venv/", "ruff", "pytest", "language: python"] {
        assert!(
            !config.contains(needle),
            ".pre-commit-config.yaml still runs {needle}"
        );
    }
    assert!(
        config.contains("cargo mutants") || config.contains("mutants-staged.sh"),
        "the mutation gate must survive the rewrite"
    );
}

/// CI builds the crate and nothing else.
#[test]
fn no_workflow_installs_python() {
    let workflows = repo_path(".github/workflows");
    let entries = std::fs::read_dir(&workflows).expect(".github/workflows must be readable");
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        for needle in ["setup-python", "pytest", "pip install", "twine"] {
            assert!(
                !text.contains(needle),
                "{path:?} still runs {needle} - 1.x CI outlived the package"
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "no workflows were checked");
}

/// The README installs what Phase 7 publishes.
///
/// `drep-ai` stays on PyPI - nothing is yanked - but 1.3.0 is the last release
/// there, so a README that still says `pip install drep-ai` hands a new user a
/// tool that predates every command it documents.
#[test]
fn the_readme_installs_the_binary() {
    let readme = std::fs::read_to_string(repo_path("README.md")).expect("README.md is readable");
    assert!(
        !readme.contains("pip install"),
        "the README still installs the PyPI package"
    );
    assert!(
        readme.contains("drep-ai-installer.sh") || readme.contains("brew install"),
        "the README must install the binary the release workflow ships"
    );
}
