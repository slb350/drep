//! Language registry: resolve a `Path` to the language that owns its extension.

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
}
