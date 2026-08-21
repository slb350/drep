//! The hooks other repositories consume, checked against the current binary.
//!
//! `.pre-commit-hooks.yaml` is the one file in this repository whose consumer
//! is somebody else's commit gate. Nothing in the Rust suite reads it, so a
//! stale flag or invalid build language fails in their repository rather than
//! ours. These assertions are deliberately textual: adding a YAML parser to
//! state four facts about a 40-line file is a dependency for no gain.

mod common;

/// The file with its comments stripped.
///
/// Comments may mention rejected alternatives, so assertions inspect only live
/// configuration.
fn hooks_yaml() -> String {
    common::without_comments(".pre-commit-hooks.yaml")
}

/// Published hooks build this repository's Rust binary.
#[test]
fn published_hooks_build_the_rust_binary() {
    let yaml = hooks_yaml();
    let languages: Vec<&str> = yaml
        .lines()
        .filter_map(|line| line.trim().strip_prefix("language: "))
        .collect();
    assert!(
        !languages.is_empty(),
        "every published hook must declare its build language"
    );
    assert!(
        languages.iter().all(|language| *language == "rust"),
        "every published hook must build the Rust binary, found {languages:?}"
    );
}

/// The markdown hook blocks on `error` severity, not on everything.
///
/// `--strict` means `--fail-on info`, which is dominated by line length and
/// trailing whitespace in real repositories. The published gate reserves
/// blocking for rendering-breaking errors.
#[test]
fn the_markdown_hook_blocks_only_on_error_severity() {
    let yaml = hooks_yaml();
    assert!(
        yaml.contains("drep lint-docs --fail-on error"),
        "the markdown hook must gate at error severity"
    );
    assert!(
        !yaml.contains("--strict"),
        "--strict blocks on info findings, which no consumer wants in a hook"
    );
}

/// pre-commit normally appends the names touched by the outgoing commits.
/// Passing those names selects drep's whole-file mode, while the native hook
/// sends the actual base and pushed tip and therefore reviews diff hunks. The
/// adapter flag reads pre-commit's ref environment; filenames must be disabled
/// so both installation paths have the same scope.
#[test]
fn the_published_pre_push_hook_uses_pre_commit_refs_not_filenames() {
    let yaml = hooks_yaml();
    let start = yaml
        .find("- id: drep-check-push\n")
        .expect("published hooks must declare drep-check-push");
    let block = &yaml[start..];
    let end = block[1..]
        .find("\n- id:")
        .map_or(block.len(), |offset| offset + 1);
    let block = &block[..end];

    assert!(
        block.contains("entry: drep check --push-gate --pre-commit-push"),
        "the published pre-push hook must ask drep to resolve pre-commit's refs: {block}"
    );
    assert!(
        block.contains("pass_filenames: false"),
        "pre-commit filenames would select whole-file review: {block}"
    );
}

#[test]
fn the_repository_pre_push_hook_uses_the_push_gate_and_ref_adapter() {
    let yaml = common::without_comments(".pre-commit-config.yaml");
    assert!(
        yaml.contains("entry: ./target/release/drep check --push-gate --pre-commit-push"),
        "drep must test the same pre-commit ref adapter it publishes"
    );
}
