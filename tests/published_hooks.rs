//! The hooks other repositories consume, checked against what 2.0 actually is.
//!
//! `.pre-commit-hooks.yaml` is the one file in this repository whose consumer
//! is somebody else's commit gate. Nothing in the Rust suite reads it, so a
//! flag that stopped being right - or a `language` naming a package that no
//! longer exists - fails in their repository rather than ours. These assertions
//! are deliberately textual: adding a YAML parser to state four facts about a
//! 40-line file is a dependency for no gain.

mod common;

/// The file with its comments stripped.
///
/// The comments explain what each setting replaced - `language: python`,
/// `--strict` - so asserting over the raw text matches the explanation of the
/// old value as if it were still in force.
fn hooks_yaml() -> String {
    common::without_comments(".pre-commit-hooks.yaml")
}

/// 2.0 is a Rust binary. `language: python` builds an isolated environment
/// from the PyPI package that Phase 8 deleted, so a consumer's hook would fail
/// at install time with a pip error naming a package this repository no longer
/// publishes.
#[test]
fn no_published_hook_installs_the_python_package() {
    let yaml = hooks_yaml();
    assert!(
        !yaml.contains("language: python"),
        "a published hook still installs drep as a Python package"
    );
    assert!(
        yaml.contains("language: rust"),
        "the published hooks must build the Rust binary"
    );
}

/// The markdown hook blocks on `error` severity, not on everything.
///
/// The entry was written against 1.x, where the doc checks carried no
/// severity, so `--strict` was the only way to make the hook block. Under the
/// 2.0 scale `--strict` means `--fail-on info`, and over a real repository
/// that is dominated by line length and trailing whitespace - measured at 75
/// findings on this tree, none above `info`. A consumer adopting that hook has
/// commits blocked by line length, and deletes the hook.
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
