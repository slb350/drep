//! `is_ignored_dir`.
//!
//! The case-insensitivity invariant matters: a directory called `VENV` and
//! one called `venv` are the same on macOS HFS+. Matching only one defeats
//! the point of having the list, which is to never stat its contents.

use crate::files::is_ignored_dir;
use crate::languages::{source_extensions, vendored_dirs};

#[test]
fn hardcoded_directories_are_ignored() {
    for name in [".git", "build", "dist", ".cache"] {
        assert!(
            is_ignored_dir(name),
            "{name} should be in the hardcoded ignored list"
        );
    }
}

#[test]
fn every_language_declared_vendored_dir_is_ignored() {
    // Driven off the registry rather than a hand-written list, so adding a
    // language covers the test the moment its `vendored_dirs` entry lands.
    for dir in vendored_dirs() {
        assert!(
            is_ignored_dir(dir),
            "vendored dir `{dir}` from the language registry must be ignored"
        );
    }
}

#[test]
fn ignored_dir_match_is_case_insensitive() {
    // The match is on the directory *name*, not how the OS path was cased.
    // A repo with `VENV/` (uppercase) on a case-insensitive filesystem is
    // still a venv directory.
    for name in ["VENV", "Node_Modules", "TARGET", "Build"] {
        assert!(
            is_ignored_dir(name),
            "{name} should match regardless of case"
        );
    }
}

#[test]
fn egg_info_dirs_are_ignored_but_other_extensions_with_egg_are_not() {
    assert!(is_ignored_dir("foo.egg-info"), "a `.egg-info` is metadata");
    assert!(is_ignored_dir("Foo.Egg-Info"), "case-insensitive");
    assert!(!is_ignored_dir("foo.egg"), "a `.egg` file is just a file");
    assert!(!is_ignored_dir("src"), "source code, not vendored");
    // Sanity: source extensions never show up as directory names because
    // they live in the registry under a different key.
    let _ = source_extensions();
}

/// A directory that another ecosystem uses for real, checked-in source is
/// never added to `vendored_dirs`, however build-generated it is in its own.
///
/// `is_ignored_dir` consults the union across every registered language, so a
/// name listed once is skipped in every repository. `bin` and `obj` were
/// briefly listed for C#, which hid `bin/deploy.sh` from Shell in a repo with
/// no C# in it - and `bin/rubocop` is a path the registry's own RuboCop spec
/// looks for. `JVM_VENDORED_DIRS` left `out` out for the same reason; these
/// are machine-generated and therefore gitignored in practice, which the
/// walker already honors without help from the registry.
#[test]
fn dirs_other_ecosystems_fill_with_real_source_are_not_ignored() {
    for name in ["bin", "obj", "out", "src", "lib", "app", "scripts", "cmd"] {
        assert!(
            !is_ignored_dir(name),
            "`{name}` holds real source somewhere and must stay walkable"
        );
    }
}
