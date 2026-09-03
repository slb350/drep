//! Language registry: resolve a `Path` to the language that owns its extension.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

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
    detect_index(path).map(|index| ALL_LANGUAGES[index])
}

/// The index of the language owning `path` within [`ALL_LANGUAGES`].
///
/// The index *is* the language's identity, which is what [`group_by_language`]
/// needs; recovering it afterwards by comparing pointers meant scanning the
/// table twice for an answer the first scan already had.
fn detect_index(path: &Path) -> Option<usize> {
    // Extension first, whole name second, and never the other way round: a
    // name match must not shadow a language that claims the suffix, or a file
    // called `Dockerfile.ts` would stop being TypeScript.
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && let Some(index) = by_extension(ext)
    {
        return Some(index);
    }
    by_filename(path.file_name()?.to_str()?)
}

/// The language claiming `ext`, which carries no leading dot.
///
/// No allocation: the table's extensions are ASCII literals that all begin
/// with a dot, so `[1..]` is the bare suffix and `eq_ignore_ascii_case` does
/// the case folding that `to_lowercase()` used to do into two throwaway
/// `String`s per call - on a function called once per walked file.
fn by_extension(ext: &str) -> Option<usize> {
    ALL_LANGUAGES.iter().position(|lang| {
        lang.extensions
            .iter()
            .any(|known| known[1..].eq_ignore_ascii_case(ext))
    })
}

/// The language claiming the whole file name `name`.
///
/// `Path::extension` answers `None` for `Dockerfile`, `Makefile`, `Gemfile`
/// and `Jenkinsfile`, so without this they are dropped at language grouping
/// and reported as a clean run - the silent pass drep exists to refuse.
/// Case-insensitive, matching the extension lookup and the rest of drep's
/// file-target policy.
fn by_filename(name: &str) -> Option<usize> {
    ALL_LANGUAGES.iter().position(|lang| {
        lang.filenames
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
    })
}

/// Bucket `paths` by the language that owns them.
///
/// The single answer to "which languages are present here, and with which
/// files". Both `drep check`'s deterministic layer (which needs the batch per
/// tool) and `drep doctor` (which needs the counts) ask it, so neither
/// re-derives language identity from a path — and neither can disagree with
/// the other about what this repository contains, which is the specific thing
/// `doctor` exists to report truthfully.
///
/// Paths that no registered language claims are dropped: drep has no opinion
/// on a file type it does not analyze. The result is ordered by
/// `ALL_LANGUAGES`' registration order rather than by name, so output is
/// stable across runs and reads in the order the language table is written.
///
/// Borrows rather than owns. The caller already holds the paths, and every
/// consumer converts them again for its own use - to argv strings in the tool
/// runner, to counts in `doctor` - so cloning into the buckets was a copy that
/// nothing read.
pub fn group_by_language<'a>(paths: &[&'a Path]) -> Vec<(&'static LanguageSupport, Vec<&'a Path>)> {
    let mut buckets: Vec<Vec<&'a Path>> = vec![Vec::new(); ALL_LANGUAGES.len()];
    for path in paths {
        if let Some(index) = detect_index(path) {
            buckets[index].push(path);
        }
    }
    buckets
        .into_iter()
        .enumerate()
        .filter(|(_, files)| !files.is_empty())
        .map(|(index, files)| (ALL_LANGUAGES[index], files))
        .collect()
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
pub fn source_extensions() -> &'static [&'static str] {
    &SOURCE_EXTENSIONS
}

/// Computed once so repeated registry introspection does not rebuild a set
/// whose answer is fixed at compile time.
static SOURCE_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
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
});

/// Every dependency/build directory any registered language creates.
///
/// Same deduplication discipline as `source_extensions`: each `LanguageSupport`
/// declares its own vendored directories, and the set is built once.
pub fn vendored_dirs() -> &'static [&'static str] {
    &VENDORED_DIRS
}

/// Computed once, for the same reason as [`SOURCE_EXTENSIONS`].
static VENDORED_DIRS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
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
});

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

    /// Project compilers run from configuration and cannot take file args.
    ///
    /// Pinned explicitly because the failure is invisible in unit tests: with
    /// `accepts_files: true`, clippy is invoked as `cargo clippy ... a.rs` and
    /// exits 1 with "unexpected argument", so drep reports every Rust file
    /// `Unavailable` and exits 2 on any Rust repository. Nothing in the suite
    /// noticed - it took running drep against its own source to find it, and a
    /// well-meant "why is this field false?" edit would put it straight back.
    #[test]
    fn project_compilers_do_not_take_file_arguments() {
        assert!(
            !definitions::CLIPPY.accepts_files,
            "cargo clippy checks a crate; a path argument is rejected outright"
        );
        assert!(
            !definitions::TSC.accepts_files,
            "passing paths makes tsc ignore tsconfig.json"
        );
        for spec in [
            &definitions::RUFF,
            &definitions::ESLINT,
            &definitions::GOFMT,
            &definitions::GO_VET,
        ] {
            assert!(
                spec.accepts_files,
                "{} is invoked with the files it should check",
                spec.name
            );
        }
    }

    #[test]
    fn only_clippy_is_serialized_within_a_repository() {
        assert!(
            definitions::CLIPPY.serial_in_repository,
            "parallel cargo processes contend for the same build lock"
        );
        for spec in [
            &definitions::RUFF,
            &definitions::ESLINT,
            &definitions::TSC,
            &definitions::GOFMT,
            &definitions::GO_VET,
        ] {
            assert!(
                !spec.serial_in_repository,
                "{} should remain eligible for bounded parallel execution",
                spec.name
            );
        }
    }

    #[test]
    fn all_languages_returns_every_registered_language() {
        let langs = all_languages();
        let names: Vec<&str> = langs.iter().map(|l| l.name).collect();
        assert_eq!(
            names,
            vec![
                "python",
                "javascript",
                "typescript",
                "go",
                "rust",
                "java",
                "kotlin",
                "scala",
                "groovy",
                "shell",
                "swift",
                "c",
                "cpp",
                "csharp",
                "ruby",
                "php",
                "vue",
                "svelte",
                "terraform",
                "elixir",
                "sql",
                "docker"
            ]
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

    /// Buckets come back in registration order, not alphabetical order, and
    /// each holds exactly its own files.
    ///
    /// Order is asserted because it is what makes `doctor`'s output stable
    /// across runs; a `BTreeMap` keyed on name would sort go before python and
    /// silently change the report.
    #[test]
    fn group_by_language_buckets_in_registration_order() {
        let paths = [
            Path::new("main.go"),
            Path::new("a.py"),
            Path::new("lib.rs"),
            Path::new("b.py"),
        ];
        let grouped = group_by_language(&paths);
        let names: Vec<&str> = grouped.iter().map(|(lang, _)| lang.name).collect();
        assert_eq!(
            names,
            vec!["python", "go", "rust"],
            "registration order (python, javascript, typescript, go, rust), \
             not alphabetical and not first-seen"
        );
        assert_eq!(
            grouped[0].1,
            vec![Path::new("a.py"), Path::new("b.py")],
            "files land in their own bucket, in the order given"
        );
    }

    /// A language with no matching files never appears, and an unrecognised
    /// extension is dropped rather than bucketed anywhere.
    #[test]
    fn group_by_language_omits_empty_buckets_and_unknown_extensions() {
        let grouped = group_by_language(&[Path::new("notes.md"), Path::new("data.xyz")]);
        assert!(
            grouped.is_empty(),
            "no registered language claims these, so there is nothing to report: {:?}",
            grouped.iter().map(|(l, _)| l.name).collect::<Vec<_>>()
        );

        let grouped = group_by_language(&[Path::new("a.py"), Path::new("notes.md")]);
        assert_eq!(grouped.len(), 1, "only python is present");
        assert_eq!(grouped[0].0.name, "python");
        assert_eq!(
            grouped[0].1,
            vec![Path::new("a.py")],
            "the markdown file is dropped, not attached to python"
        );
    }

    /// The JVM family. Java is the one that motivated it: a repository of
    /// `.java` files reported "No source files drep recognises were found
    /// here" and `drep check` exited 0 having analyzed nothing, which is the
    /// silent pass the deterministic half exists to refuse.
    #[test]
    fn jvm_extensions_resolve_to_their_languages() {
        for (path, expected) in [
            ("Main.java", "java"),
            ("Main.kt", "kotlin"),
            ("Main.kts", "kotlin"),
            ("Main.scala", "scala"),
            ("Main.sc", "scala"),
            ("Main.groovy", "groovy"),
            ("build.gradle", "groovy"),
        ] {
            let detected = detect(Path::new(path));
            assert_eq!(
                detected.map(|lang| lang.name),
                Some(expected),
                "{path} should resolve to {expected}"
            );
        }
    }

    /// `.gradle` is Groovy source that drep should read, but `.gradle.kts` is
    /// Kotlin and `Path::extension` returns `kts` for it, so the two do not
    /// collide.
    #[test]
    fn gradle_kts_is_kotlin_not_groovy() {
        assert_eq!(
            detect(Path::new("build.gradle.kts")).map(|lang| lang.name),
            Some("kotlin")
        );
    }

    #[test]
    fn jvm_build_directories_are_vendored() {
        let dirs = vendored_dirs();
        for expected in ["build", ".gradle", "target"] {
            assert!(
                dirs.contains(&expected),
                "{expected} is a JVM build output and should never be descended into, got {dirs:?}"
            );
        }
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

    /// One representative path per extension and filename the coverage
    /// expansion registered. Each is the language `detect` must answer for
    /// the deterministic layer to ever run: a dropped entry is the silent
    /// pass - analyzed as clean, having analyzed nothing - that registering
    /// the language exists to prevent.
    #[test]
    fn newly_registered_extensions_and_filenames_resolve_to_their_languages() {
        for (path, expected) in [
            ("deploy.sh", "shell"),
            ("build.bash", "shell"),
            ("App.swift", "swift"),
            ("main.c", "c"),
            ("header.h", "c"),
            ("impl.cpp", "cpp"),
            ("widget.hpp", "cpp"),
            ("Program.cs", "csharp"),
            ("app.rb", "ruby"),
            ("task.rake", "ruby"),
            ("app.gemspec", "ruby"),
            ("lib.php", "php"),
            ("Widget.vue", "vue"),
            ("Page.svelte", "svelte"),
            ("main.tf", "terraform"),
            ("vars.tfvars", "terraform"),
            ("application.ex", "elixir"),
            ("test.exs", "elixir"),
            ("query.sql", "sql"),
            ("Dockerfile", "docker"),
            ("Containerfile", "docker"),
            ("CI.dockerfile", "docker"),
            ("Gemfile", "ruby"),
            ("Rakefile", "ruby"),
        ] {
            let detected = detect(Path::new(path));
            assert_eq!(
                detected.map(|lang| lang.name),
                Some(expected),
                "{path} should resolve to {expected}"
            );
        }
    }

    /// A whole-name claim must never shadow an extension claim: `Dockerfile`
    /// is Docker's, but `Dockerfile.ts` is TypeScript, and a lookup order
    /// that tried names first would silently hand `.ts` files to hadolint's
    /// SARIF and skip tsc entirely.
    #[test]
    fn extension_wins_over_whole_name_match() {
        assert_eq!(
            detect(Path::new("Dockerfile.ts")).map(|lang| lang.name),
            Some("typescript")
        );
    }

    /// Every registered entry carries the fields the report and the prompt
    /// render. An empty `conventions` is not fatal, but it is also not what
    /// any entry here intends, and an empty `name` or `display_name` would
    /// render as a blank line in `doctor`'s listing.
    #[test]
    fn every_language_declares_a_name_a_display_name_and_conventions() {
        for lang in ALL_LANGUAGES {
            assert!(!lang.name.is_empty(), "name is empty: {lang:?}");
            assert!(
                !lang.display_name.is_empty(),
                "{} has no display name",
                lang.name
            );
            assert!(
                !lang.conventions.is_empty(),
                "{} has no conventions for the prompt",
                lang.name
            );
        }
    }

    /// No two languages may claim the same extension or filename: the lookup
    /// answers with whichever entry registered first, so a duplicate is
    /// silently won by one side and the other language's files are never
    /// analyzed by its own tools. This must be a test, not a convention,
    /// because nothing in the type system stops the collision.
    ///
    /// Compared case-insensitively, because that is how `by_extension` and
    /// `by_filename` compare. A set of the literals as written would call
    /// `.RB` and `.rb` distinct and pass, while `detect` treats them as one
    /// claim and hands every Ruby file to whichever entry came first - the
    /// exact collision this guards, invisible to the guard.
    #[test]
    fn no_two_languages_claim_the_same_extension_or_filename() {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for lang in ALL_LANGUAGES {
            for claimed in lang.extensions.iter().chain(lang.filenames.iter()) {
                assert!(
                    seen.insert(claimed.to_ascii_lowercase()),
                    "{claimed} is claimed by {} and an earlier language",
                    lang.name
                );
            }
        }
    }
}
