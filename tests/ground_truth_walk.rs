//! `walk_targets` against this actual repository.
//!
//! The unit tests use temp-dir fixtures, which prove the walk matches its spec.
//! This one proves the spec survives contact with a real tree: a 222MB `venv/`,
//! a `target/` directory, and a `.gitignore` that excludes dotted caches.
//!
//! The pruning assertions are the load-bearing ones. Descending into `venv/`
//! costs tens of thousands of syscalls and is the reason this uses the `ignore`
//! crate rather than a recursive glob.

use drep::files::{is_scan_target, walk_targets};
use std::path::Path;

#[test]
fn walk_this_repo_prunes_vendored_trees_and_respects_gitignore() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rel: Vec<String> = walk_targets(root, is_scan_target)
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

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

    // Gitignored dotted directories stay out even though hidden(false) means
    // the walker is willing to look at dotted paths. Both of these hold real
    // .md files, so only .gitignore keeps them out.
    for ignored in [".claude/", ".pytest_cache/"] {
        assert!(
            !rel.iter().any(|p| p.contains(ignored)),
            "{ignored} is gitignored but was walked"
        );
    }

    // Tracked sources of several languages are found.
    for expected in ["src/lib.rs", "drep/cli.py", "README.md"] {
        assert!(rel.iter().any(|p| p == expected), "missing {expected}");
    }

    // The deliberate asymmetry, against a real gitignored file rather than a
    // fixture: naming a path explicitly outranks a repo-wide .gitignore, so
    // `drep check .claude/foo.md` analyzes it even though the walk would not.
    let ignored = root.join(".claude/pr-reviews/pr-6-review-2025-11-08.md");
    if ignored.exists() {
        let explicit = drep::files::expand_paths(std::slice::from_ref(&ignored), is_scan_target);
        assert_eq!(
            explicit,
            vec![ignored],
            "an explicitly named gitignored file must still be honoured"
        );
    }
}
