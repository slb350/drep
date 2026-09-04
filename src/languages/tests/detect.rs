//! `detect` and `group_by_language`: path-to-language resolution.

use crate::languages::{detect, group_by_language};
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

/// A name claim must never shadow an extension claim: `Dockerfile` is
/// Docker's, but `Dockerfile.ts` is TypeScript, and a lookup order that tried
/// names first would silently hand `.ts` files to hadolint's SARIF and skip
/// tsc entirely.
///
/// This covers both name rules at once. With stems registered `Dockerfile.ts`
/// is a candidate for the stem claim as well as the exact one, so the single
/// assertion pins the extension lookup ahead of either.
#[test]
fn extension_wins_over_both_name_rules() {
    assert_eq!(
        detect(Path::new("Dockerfile.ts")).map(|lang| lang.name),
        Some("typescript")
    );
}

/// Multi-image layouts name per-environment Dockerfiles `Dockerfile.dev`,
/// `Dockerfile.prod`, `Dockerfile.web` - an unbounded family an exact-name
/// list cannot cover. The stem claim resolves them; an unclaimed one is the
/// same silent pass the exact claim was added to refuse.
#[test]
fn dockerfile_variants_resolve_to_docker() {
    for (path, expected) in [
        ("Dockerfile.dev", "docker"),
        ("Dockerfile.prod", "docker"),
        ("Dockerfile.web", "docker"),
        ("Containerfile.tests", "docker"),
        ("dockerfile.DEV", "docker"),
    ] {
        assert_eq!(
            detect(Path::new(path)).map(|lang| lang.name),
            Some(expected),
            "{path} should resolve to {expected}"
        );
    }
}

/// The stem rule's edges: a claimed stem needs a dot and a non-empty variant,
/// a name merely containing the stem is not a claim, and a language that
/// declared no stems (Ruby) gains no variants - `Gemfile.lock` is generated,
/// not source.
#[test]
fn stem_variants_respect_the_boundaries() {
    for path in [
        "Dockerfile.",      // an empty variant is not a variant
        "myDockerfile.dev", // the stem must lead the name
        "Dockerfiledev",    // no dot boundary
        "Gemfile.lock",     // Ruby claims no stems
        "Rakefile.backup",  // same
    ] {
        assert!(
            detect(Path::new(path)).is_none(),
            "{path} should not resolve to a language"
        );
    }
}

/// The whole-name lookup folds case, as documented on the registry: an
/// `eq_ignore_ascii_case` weakened to `==` would land silently, so the
/// folded forms are pinned.
#[test]
fn whole_name_match_is_case_insensitive() {
    for (path, expected) in [
        ("DOCKERFILE", "docker"),
        ("dockerfile", "docker"),
        ("gemfile", "ruby"),
        ("RAKEFILE", "ruby"),
    ] {
        assert_eq!(
            detect(Path::new(path)).map(|lang| lang.name),
            Some(expected),
            "{path} should resolve to {expected}"
        );
    }
}
