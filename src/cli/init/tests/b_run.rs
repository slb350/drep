//! B17, B18, B19: orchestration. `run_to` against fresh `git init` and
//! against a non-repo directory.

use crate::cli::init::{HookKind, InitArgs, run_with};

use super::support::auth_path;

#[tokio::test]
async fn run_to_on_a_non_repo_directory_errors_and_writes_no_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `tempfile::tempdir` lives under /tmp, which is not inside the drep
    // git repo, so `git rev-parse --show-toplevel` fails there.

    let mut out = Vec::new();
    let args = InitArgs {
        path: dir.path().to_path_buf(),
        provider: Some("local".to_owned()),
        model: None,
        endpoint: None,
        hooks: HookKind::None,
        force: false,
        // These tests predate the wizard and drive the flag path.
        no_gitignore: true,
        non_interactive: true,
        interactive: false,
    };
    let result = run_with(&mut out, &args, &auth_path(&dir)).await;
    let err = result.expect_err("non-repo returns Err");
    let msg = format!("{err:#}");
    assert!(msg.contains("not inside a git repository"), "msg: {msg}");

    let toml = dir.path().join("drep.toml");
    assert!(!toml.exists(), "no drep.toml should be written on Err");
}

#[tokio::test]
async fn custom_provider_without_endpoint_or_model_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    // No endpoint, no model.
    let mut out = Vec::new();
    let result = run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("custom".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::None,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await;
    let err = result.expect_err("custom needs --endpoint");
    let msg = format!("{err:#}");
    assert!(msg.contains("--endpoint"), "msg: {msg}");
    assert!(
        !dir.path().join("drep.toml").exists(),
        "no drep.toml should be written on Err"
    );

    // Endpoint present, no model.
    let mut out = Vec::new();
    let result = run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("custom".to_owned()),
            model: None,
            endpoint: Some("http://x/v1".to_owned()),
            hooks: HookKind::None,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await;
    let err = result.expect_err("custom needs --model");
    let msg = format!("{err:#}");
    assert!(msg.contains("--model"), "msg: {msg}");
}

#[tokio::test]
async fn run_to_with_local_and_both_hooks_writes_toml_and_hooks() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out = Vec::new();
    let result = run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("local".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::Both,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await;
    assert!(result.is_ok(), "run_to: {result:?}");

    let toml = dir.path().join("drep.toml");
    assert!(toml.exists(), "drep.toml was written");

    let pre_commit = dir.path().join(".git/hooks/pre-commit");
    let pre_push = dir.path().join(".git/hooks/pre-push");
    assert!(pre_commit.exists(), ".git/hooks/pre-commit was written");
    assert!(pre_push.exists(), ".git/hooks/pre-push was written");

    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        rendered.contains("qwen3-30b-a3b"),
        "captured output names the preset's default model; rendered:\n{rendered}"
    );
    // local preset has no api_key_env, so the export reminder must be absent.
    assert!(
        !rendered.contains("export "),
        "local preset has no api_key_env; no export reminder expected; rendered:\n{rendered}"
    );

    // B19 second half: a fresh repo with `--provider openrouter` writes the
    // export reminder naming `OPENROUTER_API_KEY`. Spec says "in the same
    // test", so the two halves share one `#[tokio::test]`.
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out = Vec::new();
    let result = run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("openrouter".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::None,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await;
    assert!(result.is_ok(), "run_to: {result:?}");

    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        rendered.contains("OPENROUTER_API_KEY"),
        "openrouter preset must name its api_key_env; rendered:\n{rendered}"
    );
}

/// An explicit `--model`/`--endpoint` wins over the preset's default.
///
/// Every existing case passed `None` for both, or used `custom` (whose preset
/// values are themselves `None`), so `args.model.or(preset.default_model)`
/// could be inverted to `preset.default_model.or(args.model)` and the suite
/// stayed green - meaning `drep init --provider local --model qwen-x` silently
/// writing `qwen3-30b-a3b` was untestable by omission.
#[tokio::test]
async fn an_explicit_model_and_endpoint_override_the_presets_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out: Vec<u8> = Vec::new();
    run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("local".to_owned()),
            model: Some("my-own-model".to_owned()),
            endpoint: Some("http://elsewhere:9999/v1".to_owned()),
            hooks: HookKind::None,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await
    .expect("run_to");

    let written = std::fs::read_to_string(dir.path().join("drep.toml")).expect("drep.toml");
    assert!(
        written.contains("model = \"my-own-model\""),
        "the flag wins over the preset default; wrote:\n{written}"
    );
    assert!(
        written.contains("endpoint = \"http://elsewhere:9999/v1\""),
        "same for the endpoint; wrote:\n{written}"
    );
    assert!(
        !written.contains("qwen3-30b-a3b") && !written.contains("localhost:1234"),
        "and the preset's own values must not appear; wrote:\n{written}"
    );
}

/// `drep.toml` and the hooks land at the **git toplevel**, not at `--path`.
///
/// Every other test passes the repository root as `--path`, so `args.path ==
/// root` always and `config_file::write(&args.path, ..)` would pass the whole
/// suite - while `drep init --path src/` scattered a config into a
/// subdirectory where nothing would ever look for it.
#[tokio::test]
async fn init_writes_at_the_git_toplevel_not_at_the_path_argument() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    let nested = dir.path().join("src").join("deep");
    std::fs::create_dir_all(&nested).expect("nested dir");

    let mut out: Vec<u8> = Vec::new();
    run_with(
        &mut out,
        &InitArgs {
            path: nested.clone(),
            provider: Some("local".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::PrePush,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await
    .expect("run_to");

    // `canonicalize` because macOS reports /var as a symlink to /private/var,
    // and the toplevel git prints is the resolved form.
    let root = dir.path().canonicalize().expect("canonical");
    assert!(
        root.join("drep.toml").is_file(),
        "the config belongs at the repository root"
    );
    assert!(
        !nested.join("drep.toml").exists(),
        "and not beside the path the user happened to name"
    );
    assert!(
        root.join(".git").join("hooks").join("pre-push").is_file(),
        "the hook belongs in the repository's hooks dir"
    );
}

/// `--hooks none` writes the config and touches nothing else.
///
/// The documented contract is that it "must not create directories or ask git
/// anything" - an escape hatch with side effects is not one. Deleting the
/// early return passed every test, because nothing asserted on `.git/hooks`
/// after a `None` run.
#[tokio::test]
async fn hooks_none_writes_the_config_and_installs_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut out: Vec<u8> = Vec::new();
    run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("local".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::None,
            force: false,
            // These tests predate the wizard and drive the flag path.
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await
    .expect("run_to");

    let root = dir.path().canonicalize().expect("canonical");
    assert!(
        root.join("drep.toml").is_file(),
        "the config is still written"
    );
    for name in ["pre-push", "pre-commit"] {
        assert!(
            !root.join(".git").join("hooks").join(name).exists(),
            "--hooks none must install no {name}"
        );
    }
}

/// A provider needing no key produces no environment section at all.
///
/// Deleting the emptiness guard would print the heading with nothing under it,
/// which reads as "something is missing" for a setup that is complete. Asserted
/// alongside the positive case, because either alone passes with the guard
/// inverted.
#[tokio::test]
async fn the_environment_section_appears_only_when_a_variable_is_needed() {
    async fn report(provider: &str) -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::test_support::git_init(dir.path());
        let mut out = Vec::new();
        run_with(
            &mut out,
            &InitArgs {
                path: dir.path().to_path_buf(),
                provider: Some(provider.to_owned()),
                model: None,
                endpoint: None,
                hooks: HookKind::None,
                force: false,
                no_gitignore: true,
                non_interactive: true,
                interactive: false,
            },
            &auth_path(&dir),
        )
        .await
        .expect("init succeeds");
        String::from_utf8(out).expect("utf8")
    }

    const HEADING: &str = "reads its key from the environment";

    let local = report("local").await;
    assert!(
        !local.contains(HEADING),
        "a local server needs no variable, so there is nothing to head: {local}"
    );

    let cloud = report("openrouter").await;
    assert!(
        cloud.contains(HEADING),
        "and a cloud provider does: {cloud}"
    );
    assert!(cloud.contains("OPENROUTER_API_KEY"), "named: {cloud}");
}

/// A key already held suppresses the environment section entirely.
///
/// The rendered block carries no `api_key` line in that case, so the
/// environment is never consulted and naming a variable would be misleading.
#[tokio::test]
async fn a_stored_key_leaves_the_environment_unmentioned() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let mut store = crate::auth::AuthStore::new();
    store
        .set("https://openrouter.ai/api/v1", "sk-held")
        .expect("set");
    store.save(&auth_path(&dir)).expect("save");

    let mut out = Vec::new();
    run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("openrouter".to_owned()),
            model: None,
            endpoint: None,
            hooks: HookKind::None,
            force: false,
            no_gitignore: true,
            non_interactive: true,
            interactive: false,
        },
        &auth_path(&dir),
    )
    .await
    .expect("init succeeds");

    let rendered = String::from_utf8(out).expect("utf8");
    assert!(
        !rendered.contains("OPENROUTER_API_KEY"),
        "the key is held, so the variable is irrelevant: {rendered}"
    );

    let config = std::fs::read_to_string(dir.path().join("drep.toml")).expect("config");
    // By line, not substring: the file's own header comment explains the
    // `api_key = "${VAR}"` escape hatch, so a naive `contains` matches the
    // documentation rather than an assignment.
    assert!(
        !config
            .lines()
            .any(|line| line.trim_start().starts_with("api_key")),
        "no api_key assignment, which would override the stored key: {config}"
    );
}
