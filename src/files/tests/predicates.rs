//! `is_scan_target`, `is_python_source`, `is_markdown`.
//!
//! Pins the case-insensitive suffix contract every discovery path shares:
//! `FOO.PY` is a target, `Makefile` is not, `.gitignore` does not panic.

use std::path::Path;

use crate::files::{is_markdown, is_python_source, is_scan_target};

/// Every extension a registered language owns, plus markdown for the
/// documentation analyzer. The single source for positive cases in
/// `every_registered_extension_and_markdown_is_a_scan_target`.
const REGISTERED_EXTENSIONS: &[&str] = &[
    ".py", ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".go", ".rs", ".md",
];

#[test]
fn every_registered_extension_and_markdown_is_a_scan_target() {
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
    // every letter capitalised is still a scan target, and so is `README.MD`.
    assert!(is_scan_target(Path::new("FOO.PY")));
    assert!(is_scan_target(Path::new("README.MD")));
    assert!(is_scan_target(Path::new("Main.Go")));
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
fn is_python_source_is_python_only() {
    assert!(is_python_source(Path::new("foo.py")));
    assert!(is_python_source(Path::new("FOO.PY")));
    assert!(!is_python_source(Path::new("foo.rs")));
    assert!(!is_python_source(Path::new("foo.md")));
    assert!(!is_python_source(Path::new("Makefile")));
}

#[test]
fn is_markdown_is_markdown_only() {
    assert!(is_markdown(Path::new("README.md")));
    assert!(is_markdown(Path::new("README.MD")));
    assert!(!is_markdown(Path::new("foo.py")));
    assert!(!is_markdown(Path::new("notes.txt")));
}
