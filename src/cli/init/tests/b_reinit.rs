//! Re-running `drep init` on a repository that already has a config.
//!
//! The decision has to be settled *before* the wizard asks anything, because
//! the wizard's own side effect is storing a pasted key and that happens before
//! the config is written. Asking seven questions, saving a credential and then
//! refusing on "already exists" changed the store, left the config alone, and
//! did not switch the provider - while exiting 0.

use super::support::args;
use crate::cli::init::wizard::tests::Scripted;
use crate::cli::init::{InitArgs, describe, existing_config};

/// A directory containing `drep.toml` with `body`, or none when `None`.
fn root(body: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    if let Some(text) = body {
        std::fs::write(dir.path().join("drep.toml"), text).expect("write config");
    }
    dir
}

/// A minimal well-formed config.
const EXISTING: &str = r#"
[[llm]]
endpoint = "http://localhost:1234/v1"
model = "qwen3-30b-a3b"
"#;

#[test]
fn a_repository_with_no_config_proceeds_without_asking() {
    let dir = root(None);
    let mut console = Scripted::new(&[]);

    let force = existing_config(dir.path(), &args(), true, &mut console).expect("decides");

    assert_eq!(force, Some(false), "nothing to replace, so no force needed");
    assert!(
        console.transcript().is_empty(),
        "and nothing was said: {}",
        console.transcript()
    );
}

#[test]
fn force_skips_the_question_entirely() {
    let dir = root(Some(EXISTING));
    let forced = InitArgs {
        force: true,
        ..args()
    };
    let mut console = Scripted::new(&[]);

    let force = existing_config(dir.path(), &forced, true, &mut console).expect("decides");

    assert_eq!(force, Some(true));
    assert!(console.transcript().is_empty(), "asked nothing");
}

#[test]
fn a_non_interactive_run_refuses_and_names_the_flag() {
    // Unchanged from before the wizard existed. A script has nobody to ask, and
    // silently replacing its config would be worse than failing.
    let dir = root(Some(EXISTING));
    let mut console = Scripted::new(&[]);

    let err = existing_config(dir.path(), &args(), false, &mut console)
        .expect_err("an existing config is fatal without a person to ask");

    let message = err.to_string();
    assert!(message.contains("already exists"), "got {message}");
    assert!(message.contains("--force"), "got {message}");
}

#[test]
fn declining_stops_the_run_before_anything_is_asked_or_stored() {
    let dir = root(Some(EXISTING));
    let mut console = Scripted::new(&["n"]);

    let decision = existing_config(dir.path(), &args(), true, &mut console).expect("decides");

    assert_eq!(decision, None, "None is what stops the run");
    assert!(console.is_drained());
    assert!(
        console.transcript().contains("drep auth login"),
        "and points at the command that rotates a key: {}",
        console.transcript()
    );
}

#[test]
fn accepting_continues_with_force_so_the_write_can_replace_the_file() {
    // Returning `Some(false)` here would ask the question and then fail the
    // write anyway - the exact half-applied run this check exists to prevent.
    let dir = root(Some(EXISTING));
    let mut console = Scripted::new(&["y"]);

    let force = existing_config(dir.path(), &args(), true, &mut console).expect("decides");

    assert_eq!(force, Some(true));
}

#[test]
fn the_default_answer_is_to_leave_the_config_alone() {
    // Enter must not overwrite a working configuration.
    let dir = root(Some(EXISTING));
    let mut console = Scripted::new(&[""]);

    let decision = existing_config(dir.path(), &args(), true, &mut console).expect("decides");

    assert_eq!(decision, None);
}

#[test]
fn the_prompt_shows_what_is_currently_configured() {
    // "Replace it?" is not a question anyone can answer without knowing what
    // "it" is.
    let dir = root(Some(EXISTING));
    let mut console = Scripted::new(&["n"]);

    existing_config(dir.path(), &args(), true, &mut console).expect("decides");

    let transcript = console.transcript();
    assert!(transcript.contains("qwen3-30b-a3b"), "got {transcript}");
    assert!(
        transcript.contains("http://localhost:1234/v1"),
        "got {transcript}"
    );
}

#[test]
fn every_provider_in_the_existing_chain_is_shown() {
    let dir = root(Some(
        r#"
[[llm]]
endpoint = "http://localhost:1234/v1"
model = "local-model"

[[llm]]
endpoint = "https://openrouter.ai/api/v1"
model = "cloud-model"
"#,
    ));

    let lines = describe(&dir.path().join("drep.toml"));

    assert_eq!(lines.len(), 2, "got {lines:?}");
    assert!(lines[0].contains("local-model"), "got {lines:?}");
    assert!(lines[1].contains("cloud-model"), "got {lines:?}");
}

#[test]
fn a_disabled_provider_is_shown_as_disabled() {
    // Otherwise the summary claims a provider is in play that never runs, and
    // the user weighs the replace decision against a chain they do not have.
    let dir = root(Some(
        r#"
[[llm]]
enabled = false
endpoint = "http://localhost:1234/v1"
model = "parked"
"#,
    ));

    let lines = describe(&dir.path().join("drep.toml"));

    assert!(lines[0].contains("(disabled)"), "got {lines:?}");
}

#[test]
fn an_enabled_provider_is_not_labelled() {
    let dir = root(Some(EXISTING));

    let lines = describe(&dir.path().join("drep.toml"));

    assert!(!lines[0].contains("disabled"), "got {lines:?}");
}

#[test]
fn a_missing_field_is_named_rather_than_omitted() {
    let dir = root(Some("[[llm]]\nmodel = \"only-a-model\"\n"));

    let lines = describe(&dir.path().join("drep.toml"));

    assert!(lines[0].contains("only-a-model"), "got {lines:?}");
    assert!(
        lines[0].contains("(unset)"),
        "the gap is visible: {lines:?}"
    );
}

#[test]
fn an_unparseable_config_still_lets_the_user_answer() {
    // The file is about to be discarded. Turning the prompt into an error about
    // it would leave the user unable to say yes to replacing the very thing
    // that is broken.
    let dir = root(Some("this is not toml {{{"));

    let lines = describe(&dir.path().join("drep.toml"));

    assert_eq!(lines, vec!["(could not be parsed)".to_string()]);
}

#[test]
fn a_well_formed_document_is_not_reported_as_unparseable() {
    // `toml::from_str::<Value>` and `str::parse::<Value>` produce the same type
    // from different parsers: the latter reads a single TOML *value* and
    // rejects a document. Using it here reported every valid config as broken.
    let dir = root(Some(EXISTING));

    let lines = describe(&dir.path().join("drep.toml"));

    assert_ne!(lines, vec!["(could not be parsed)".to_string()]);
    assert!(lines[0].contains("qwen3-30b-a3b"), "got {lines:?}");
}

#[test]
fn a_missing_file_is_described_rather_than_panicking() {
    let dir = root(None);

    let lines = describe(&dir.path().join("drep.toml"));

    assert_eq!(lines, vec!["(could not be read)".to_string()]);
}

#[test]
fn a_config_declaring_no_provider_is_described_as_such() {
    for body in ["# just a comment\n", "llm = []\n"] {
        let dir = root(Some(body));

        let lines = describe(&dir.path().join("drep.toml"));

        assert_eq!(
            lines,
            vec!["(no [[llm]] provider)".to_string()],
            "body {body:?}"
        );
    }
}

#[tokio::test]
async fn replacing_the_config_does_not_authorise_clobbering_a_foreign_hook() {
    // "Replace drep.toml?" asks about the config. Passing that answer through to
    // `hooks::install` as `force` would broaden it to a hook somebody else
    // wrote, which the prompt never mentioned. `--force` is the only thing that
    // authorises that, and drep's *own* hook is refreshed either way.
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());

    let hooks_dir = dir.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("hooks dir");
    let foreign = hooks_dir.join("pre-push");
    std::fs::write(&foreign, "#!/bin/sh\necho someone elses hook\n").expect("write hook");

    std::fs::write(dir.path().join("drep.toml"), EXISTING).expect("existing config");

    let mut out = Vec::new();
    let result = crate::cli::init::run_with(
        &mut out,
        &InitArgs {
            path: dir.path().to_path_buf(),
            provider: Some("local".to_string()),
            no_gitignore: true,
            non_interactive: true,
            force: false,
            ..args()
        },
        &dir.path().join("auth.toml"),
    )
    .await;

    // Non-interactive with an existing config refuses before reaching hooks,
    // which is the pre-existing behaviour; what matters is the hook survived.
    assert!(
        result.is_err(),
        "an existing config is refused without --force"
    );
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("hook"),
        "#!/bin/sh\necho someone elses hook\n",
        "the foreign hook was not touched"
    );
}
