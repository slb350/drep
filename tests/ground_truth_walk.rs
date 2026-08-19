//! `walk_targets` against this actual repository.
//!
//! The unit tests use temp-dir fixtures, which prove the walk matches its spec.
//! This one proves the spec survives contact with a real tree: a 222MB `venv/`,
//! a `target/` directory, and a `.gitignore` that excludes dotted caches.
//!
//! The pruning assertions are the load-bearing ones. Descending into `venv/`
//! costs tens of thousands of syscalls and is the reason this uses the `ignore`
//! crate rather than a recursive glob.
//!
//! Only properties that need a *real* tree live here. The explicit-path
//! asymmetry (naming a gitignored file outranks the walk) used to have a copy
//! in this file, pointed at a review document under the gitignored `.claude/`
//! and wrapped in `if exists()`. `.claude/` is not tracked, so on any fresh
//! clone - CI included - the guard was false and the test asserted nothing.
//! `files::tests::expand_paths::explicit_filenames_are_honoured_even_when_gitignored`
//! covers it hermetically, which is where a property that needs no real tree
//! belongs.

use drep::files::{is_markdown, is_scan_target, walk_targets};
use std::path::Path;

/// Walk this repository with `predicate`, as repo-relative strings.
fn walk(predicate: fn(&Path) -> bool) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    walk_targets(root, predicate)
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn walk_this_repo_prunes_vendored_trees_and_respects_gitignore() {
    let rel = walk(is_scan_target);

    assert!(!rel.is_empty(), "walked nothing at all");

    // Never descended into. venv/ alone holds ~222MB of .py files, so a leak
    // here is both a correctness bug and a performance cliff.
    for pruned in ["venv/", "target/", "node_modules/", ".git/"] {
        let leaked: Vec<&String> = rel.iter().filter(|p| p.contains(pruned)).collect();
        assert!(
            leaked.is_empty(),
            "descended into {pruned}: {:?}",
            &leaked[..leaked.len().min(3)]
        );
    }

    // Tracked sources of several languages are found. Markdown is *not* a
    // scan target - `check` and `lint-docs` own disjoint file classes - so
    // this list is code only, and the assertion below states the absence
    // rather than leaving it to be inferred from a missing entry.
    for expected in ["src/lib.rs", "drep/cli.py"] {
        assert!(rel.iter().any(|p| p == expected), "missing {expected}");
    }
    assert!(
        !rel.iter().any(|p| p.ends_with(".md")),
        "markdown belongs to lint-docs, not to check"
    );
}

#[test]
fn the_markdown_walk_finds_this_repos_docs_and_respects_gitignore() {
    // The other half of the split, and the one that carries the gitignore
    // assertions: `.claude/` and `.pytest_cache/` hold real `.md` files, so
    // only `.gitignore` keeps them out of this walk. Asserting that against
    // `is_scan_target` stopped meaning anything once markdown left it.
    let rel = walk(is_markdown);

    // Note what is *not* here: `CLAUDE.md` is gitignored in this repository,
    // so the walk correctly skips it and `drep lint-docs` says nothing about
    // it unless the user names it. That is the same asymmetry the explicit
    // -path test below pins, seen from the walk's side.
    for expected in ["README.md", "CHANGELOG.md", "docs/rust-migration.md"] {
        assert!(rel.iter().any(|p| p == expected), "missing {expected}");
    }
    for ignored in [".claude/", ".pytest_cache/"] {
        assert!(
            !rel.iter().any(|p| p.contains(ignored)),
            "{ignored} is gitignored but was walked"
        );
    }
    for pruned in ["venv/", "target/", "node_modules/", ".git/"] {
        assert!(
            !rel.iter().any(|p| p.contains(pruned)),
            "descended into {pruned}"
        );
    }
    assert!(
        rel.iter().all(|p| p.ends_with(".md")),
        "the markdown walk returned a non-markdown file"
    );
}
