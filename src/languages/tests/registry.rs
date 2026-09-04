//! Registry-wide contracts: the exact language list, per-entry well-formedness,
//! and the tool-spec pins nothing else can see.

use crate::languages::definitions;
use crate::languages::spec;
use crate::languages::{all_languages, source_extensions, vendored_dirs};
use std::collections::BTreeSet;

#[test]
fn tsc_stream_is_stdout_go_vet_stream_is_stderr() {
    assert_eq!(
        definitions::TSC.diagnostics_stream,
        spec::DiagnosticsStream::Stdout
    );
    assert_eq!(
        definitions::GO_VET.diagnostics_stream,
        spec::DiagnosticsStream::Stderr
    );
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

/// tflint and `dotnet format` are project-level tools too: both reject or
/// ignore file arguments, so they run bare and `retain_requested` narrows
/// their findings.
///
/// tflint's failure is the nastier one and earned the pin: handed
/// `main.tf`, v0.47+ exits 1 emitting a SARIF `tflint-errors` run whose
/// result says "Command line arguments support was dropped in v0.47. Use
/// --chdir or --filter instead.". That result has no location, and a
/// locationless SARIF result now fails the run as `Unavailable` - but the
/// phantom finding it used to become is why this is pinned rather than
/// rediscovered.
#[test]
fn whole_project_linters_do_not_take_file_arguments() {
    assert!(
        !definitions::TFLINT.accepts_files,
        "tflint dropped positional file arguments in v0.47"
    );
    assert!(
        !definitions::DOTNET_FORMAT.accepts_files,
        "dotnet format checks a project, not a file list"
    );
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
            "vue",
            "svelte",
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

/// Every registered entry carries the fields the report and the prompt
/// render. An empty `conventions` is not fatal, but it is also not what
/// any entry here intends, and an empty `name` or `display_name` would
/// render as a blank line in `doctor`'s listing.
#[test]
fn every_language_declares_a_name_a_display_name_and_conventions() {
    for lang in all_languages() {
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
    for lang in all_languages() {
        for claimed in lang.extensions.iter().chain(lang.filenames.iter()) {
            assert!(
                seen.insert(claimed.to_ascii_lowercase()),
                "{claimed} is claimed by {} and an earlier language",
                lang.name
            );
        }
    }
}

/// No stem may be a prefix of another. `by_filename_stem` answers with the
/// first registered match, so two stems that can claim one name leave the
/// later one unreachable - the same collision class as the exact-name rule
/// above, one field over.
///
/// Prefix rather than equality, which is stricter than the lookup strictly
/// needs: the split is on the first dot, so `Docker` no longer claims
/// `Dockerfile.dev` and only an exact duplicate collides today. The stricter
/// rule is kept because the near-miss is what a new registration gets wrong,
/// and it costs nothing to refuse.
#[test]
fn no_stem_is_a_prefix_of_another_stem() {
    // Lowercased once into the vector rather than four times per pair inside
    // the nested loop.
    let stems: Vec<String> = all_languages()
        .iter()
        .flat_map(|lang| lang.filename_prefixes.iter())
        .map(|stem| stem.to_ascii_lowercase())
        .collect();
    for (i, shorter) in stems.iter().enumerate() {
        for longer in &stems[i + 1..] {
            assert!(
                !longer.starts_with(shorter) && !shorter.starts_with(longer),
                "stems {shorter:?} and {longer:?} overlap; the first registered would win"
            );
        }
    }
}

/// The contracts the lookups and the runner rely on that the type system
/// cannot state, asserted over every entry rather than by convention:
///
/// - `by_extension` slices `known[1..]`, so an extension literal that is
///   empty or missing its leading dot panics every `detect` call; one with
///   a second dot can never match, because `Path::extension` stops at the
///   last.
/// - Extensions and filenames are ASCII, because both lookups compare with
///   `eq_ignore_ascii_case`; a non-ASCII literal would never fold.
/// - A `config_flag` with no `config_files` is silently dead: the flag is
///   only appended for a file the list discovered.
/// - `timeout_secs: 0` is an instant timeout - the LLM-side equivalent is
///   rejected at load for the same reason.
/// - An empty `command` degrades to a misleading "not found" instead of
///   naming the real mistake.
/// - A `vendored_dirs` entry must be a single component, because
///   `is_ignored_dir` compares it against one directory name at a time; a
///   multi-component literal is dead data.
#[test]
fn every_registered_entry_is_well_formed() {
    for lang in all_languages() {
        for ext in lang.extensions {
            assert!(
                ext.len() > 1 && ext.starts_with('.') && !ext[1..].contains('.'),
                "{}: extension {ext:?} must be a single non-empty suffix with one leading dot",
                lang.name
            );
            assert!(
                ext.is_ascii(),
                "{}: extension {ext:?} must be ASCII to case-fold",
                lang.name
            );
        }
        for name in lang.filenames {
            assert!(
                !name.is_empty() && name.is_ascii(),
                "{}: filename {name:?} must be non-empty ASCII",
                lang.name
            );
        }
        for stem in lang.filename_prefixes {
            assert!(
                !stem.is_empty() && stem.is_ascii() && !stem.contains('.'),
                "{}: filename stem {stem:?} must be non-empty ASCII without a dot",
                lang.name
            );
        }
        for dir in lang.vendored_dirs {
            assert!(
                !dir.is_empty() && !dir.contains('/') && !dir.contains('\\'),
                "{}: vendored dir {dir:?} is compared against single path components",
                lang.name
            );
        }
        for tool in lang.tools {
            assert!(!tool.name.is_empty(), "{}: tool name is empty", lang.name);
            assert!(
                !tool.command.is_empty(),
                "{}: {} has an empty command",
                lang.name,
                tool.name
            );
            assert!(
                tool.timeout_secs > 0,
                "{}: {} has a zero timeout, which expires every run immediately",
                lang.name,
                tool.name
            );
            if tool.config_flag.is_some() {
                assert!(
                    !tool.config_files.is_empty(),
                    "{}: {} has a config flag but no config files to hand it",
                    lang.name,
                    tool.name
                );
            }
        }
    }
}

/// The formats that skip unrecognised input are exactly the three whose tools
/// interleave chatter among diagnostics. The runner's exit-status guard keys
/// on this set: widening it to a JSON format would report a legitimately
/// empty clean run as `Unavailable`, and narrowing it reopens the silent
/// clean pass the guard exists to close.
#[test]
fn only_the_line_oriented_parsers_skip_unmatched_input() {
    use spec::OutputFormat;
    for (format, skips) in [
        (OutputFormat::Lines, false),
        (OutputFormat::Json, false),
        (OutputFormat::Position, true),
        (OutputFormat::Tsc, true),
        (OutputFormat::Cargo, false),
        (OutputFormat::Sarif, false),
        (OutputFormat::Ktlint, false),
        (OutputFormat::Shellcheck, false),
        (OutputFormat::Rubocop, false),
        (OutputFormat::Phpcs, false),
        (OutputFormat::Credo, false),
        (OutputFormat::Sqlfluff, false),
        (OutputFormat::Msbuild, true),
    ] {
        assert_eq!(format.skips_unmatched_input(), skips, "{format:?}");
    }
}

/// cppcheck exits 0 even when it reports findings unless told otherwise,
/// which leaves the runner's exit-status guard inoperative for it: a release
/// that moved or renamed its SARIF stream would read as a clean pass
/// forever. The flag makes a finding run exit non-zero, so that drift is
/// `Unavailable` instead.
#[test]
fn cppcheck_findings_make_the_exit_nonzero() {
    assert!(
        definitions::CPPCHECK
            .command
            .contains(&"--error-exitcode=2"),
        "without --error-exitcode cppcheck exits 0 with findings and the guard never fires"
    );
}

/// Bare tflint lints only the module in its cwd: a commit touching
/// `modules/*/` produced no findings and passed the deterministic layer
/// silently. The recursive run descends and emits cwd-relative uris, which
/// the canonical comparison in `retain_requested` narrows back to the
/// requested files.
#[test]
fn tflint_inspects_nested_modules() {
    assert!(
        definitions::TFLINT.command.contains(&"--recursive"),
        "without --recursive, nested Terraform modules are never linted"
    );
}
