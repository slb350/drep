//! `is_scan_target` and `is_markdown`.
//!
//! Pins the case-insensitive suffix contract every discovery path shares:
//! `FOO.PY` is a target, `Makefile` is not, `.gitignore` does not panic.

use std::path::Path;

use crate::files::{is_markdown, is_scan_target};

/// Every extension a registered language owns.
///
/// Markdown is deliberately absent: `check` and `lint-docs` own disjoint file
/// classes, so that a file the user names is either analyzed or reported as
/// unsupported - never accepted and silently dropped. See
/// `the_two_file_classes_are_disjoint` below.
const REGISTERED_EXTENSIONS: &[&str] = &[
    ".py", ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".go", ".rs",
];

#[test]
fn every_registered_extension_is_a_scan_target() {
    for ext in REGISTERED_EXTENSIONS {
        let path_string = format!("foo{ext}");
        let path = Path::new(&path_string);
        assert!(
            is_scan_target(path),
            "expected `{ext}` to be a scan target, but is_scan_target returned false"
        );
    }
}

#[test]
fn the_hand_written_list_matches_the_language_registry() {
    // The list above is hand-written so the positive cases are independent of
    // production, but a language added to the registry must not leave it
    // stale - a shorter list would quietly stop testing the new extension.
    let mut registered: Vec<String> = crate::languages::source_extensions()
        .iter()
        .map(|e| e.to_string())
        .collect();
    registered.sort();
    let mut ours: Vec<String> = REGISTERED_EXTENSIONS
        .iter()
        .map(|e| e.to_string())
        .collect();
    ours.sort();
    assert_eq!(ours, registered);
}

#[test]
fn the_two_file_classes_are_disjoint() {
    // `check` reads `is_scan_target`, `lint-docs` reads `is_markdown`. If a
    // path satisfied both, "which command owns this file" would have two
    // answers and the unsupported-path failure could never be raised for it -
    // which is precisely how `drep check README.md` reported a file it had
    // declined to analyze as clean.
    for ext in REGISTERED_EXTENSIONS {
        let name = format!("foo{ext}");
        assert!(!is_markdown(Path::new(&name)), "{ext}");
    }
    assert!(!is_scan_target(Path::new("README.md")));
    assert!(!is_scan_target(Path::new("README.MD")));
    assert!(is_markdown(Path::new("README.md")));
}

#[test]
fn extensions_not_owned_by_any_language_or_the_documentation_analyzer_are_not_targets() {
    let path_txt = Path::new("notes.txt");
    let path_lock = Path::new("Cargo.lock");
    let path_png = Path::new("logo.png");
    let path_makefile = Path::new("Makefile");

    assert!(!is_scan_target(path_txt), ".txt is not a scan target");
    assert!(!is_scan_target(path_lock), ".lock is not a scan target");
    assert!(!is_scan_target(path_png), ".png is not a scan target");
    assert!(
        !is_scan_target(path_makefile),
        "an extensionless name is not a scan target"
    );
}

#[test]
fn scan_target_match_is_case_insensitive() {
    // The contract is "no surprises about case": a Python file named with
    // every letter capitalised is still a scan target, and `README.MD` is
    // still markdown.
    assert!(is_scan_target(Path::new("FOO.PY")));
    assert!(is_scan_target(Path::new("Main.Go")));
    assert!(is_markdown(Path::new("README.MD")));
}

#[test]
fn a_dotfile_with_no_extension_is_not_a_target_and_does_not_panic() {
    // `.gitignore` parses to no extension. The previous behaviour treated any
    // bare path as "no source found" silently; the panic guard here exists
    // because `Path::extension` returning `None` is easy to mishandle.
    assert!(!is_scan_target(Path::new(".gitignore")));
    assert!(!is_scan_target(Path::new("..")));
}

#[test]
fn is_markdown_is_markdown_only() {
    assert!(is_markdown(Path::new("README.md")));
    assert!(is_markdown(Path::new("README.MD")));
    assert!(!is_markdown(Path::new("foo.py")));
    assert!(!is_markdown(Path::new("notes.txt")));
}
