//! Language registry: resolve a `Path` to the language that owns its extension.

use std::collections::BTreeSet;
use std::path::Path;

pub mod definitions;
pub mod runner;
pub mod spec;

// One public path per type, matching `analysis`: no facade re-exports, so
// consumers cannot drift between `languages::ToolSpec` and
// `languages::spec::ToolSpec`.
use definitions::ALL_LANGUAGES;
use spec::LanguageSupport;

/// The language owning `path`, or `None` if drep does not analyze it.
///
/// Case-insensitive, matching the rest of drep's file-target policy.
pub fn detect(path: &Path) -> Option<&'static LanguageSupport> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let dotted = format!(".{ext}");
    ALL_LANGUAGES
        .iter()
        .copied()
        .find(|lang| lang.extensions.contains(&dotted.as_str()))
}

/// Every registered language, in registration order.
pub fn all_languages() -> &'static [&'static LanguageSupport] {
    ALL_LANGUAGES
}

/// Every extension any registered language claims, lowercased, with the dot.
///
/// Derived from `ALL_LANGUAGES` so adding a language automatically widens the
/// scan target set. Duplicates collapse: JavaScript and TypeScript both own
/// `.ts`-adjacent extensions, so a hand-written list here would silently need
/// to track them.
pub fn source_extensions() -> Vec<&'static str> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for lang in ALL_LANGUAGES {
        for ext in lang.extensions {
            if seen.insert(*ext) {
                out.push(*ext);
            }
        }
    }
    out
}

/// Every dependency/build directory any registered language creates.
///
/// Same deduplication discipline as `source_extensions`: each `LanguageSupport`
/// declares its own vendored directories, and the set is built once.
pub fn vendored_dirs() -> Vec<&'static str> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for lang in ALL_LANGUAGES {
        for dir in lang.vendored_dirs {
            if seen.insert(*dir) {
                out.push(*dir);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detect_python() {
        assert_eq!(detect(Path::new("foo.py")).map(|l| l.name), Some("python"));
    }

    #[test]
    fn detect_javascript() {
        assert_eq!(
            detect(Path::new("foo.js")).map(|l| l.name),
            Some("javascript")
        );
    }

    #[test]
    fn detect_typescript() {
        assert_eq!(
            detect(Path::new("foo.ts")).map(|l| l.name),
            Some("typescript")
        );
    }

    #[test]
    fn detect_go() {
        assert_eq!(detect(Path::new("foo.go")).map(|l| l.name), Some("go"));
    }

    #[test]
    fn detect_rust() {
        assert_eq!(detect(Path::new("foo.rs")).map(|l| l.name), Some("rust"));
    }

    #[test]
    fn unknown_extension_returns_none() {
        assert!(detect(Path::new("foo.xyz")).is_none());
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(detect(Path::new("FOO.PY")).map(|l| l.name), Some("python"));
        assert_eq!(detect(Path::new("Mixed.Go")).map(|l| l.name), Some("go"));
    }

    #[test]
    fn tsc_stream_is_stdout_go_vet_stream_is_stderr() {
        assert_eq!(definitions::TSC.diagnostics_stream, "stdout");
        assert_eq!(definitions::GO_VET.diagnostics_stream, "stderr");
    }

    #[test]
    fn all_languages_returns_every_registered_language() {
        let langs = all_languages();
        let names: Vec<&str> = langs.iter().map(|l| l.name).collect();
        assert_eq!(
            names,
            vec!["python", "javascript", "typescript", "go", "rust"]
        );
    }

    #[test]
    fn source_extensions_contains_python_and_tsx_but_not_markdown() {
        let exts = source_extensions();
        assert!(
            exts.contains(&".py"),
            "`.py` is owned by python, expected in source_extensions, got {exts:?}"
        );
        assert!(
            exts.contains(&".tsx"),
            "`.tsx` is owned by typescript, expected in source_extensions, got {exts:?}"
        );
        assert!(
            !exts.contains(&".md"),
            "markdown is documentation, not a registered language: {exts:?}"
        );
    }

    #[test]
    fn vendored_dirs_collects_unique_entries_across_languages() {
        let dirs = vendored_dirs();
        for expected in ["node_modules", "venv", "target"] {
            assert!(
                dirs.contains(&expected),
                "{expected} should be in vendored_dirs, got {dirs:?}"
            );
        }
        let count = dirs.iter().filter(|d| **d == "node_modules").count();
        assert_eq!(
            count, 1,
            "JavaScript and TypeScript both declare node_modules; the set must collapse"
        );
    }
}
