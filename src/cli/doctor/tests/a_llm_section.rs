//! A5, A6, A7, A8, A9: every shape of the LLM section.

use crate::cli::doctor::{DoctorArgs, run_to};
use std::path::Path;

fn args(dir: &Path) -> DoctorArgs {
    DoctorArgs {
        path: dir.to_path_buf(),
        config: None,
    }
}

fn args_with_config(dir: &Path, config: std::path::PathBuf) -> DoctorArgs {
    DoctorArgs {
        path: dir.to_path_buf(),
        config: Some(config),
    }
}

fn write_py(dir: &Path) {
    std::fs::write(dir.join("a.py"), "x = 1\n").expect("a.py");
}

#[test]
fn no_config_file_prints_the_unconfigured_message_and_still_prints_tools() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    let root = dir.path().canonicalize().expect("canonical");
    let expected_path = root.join("drep.toml");
    assert!(
        rendered.contains("No config file at"),
        "rendered:\n{rendered}"
    );
    assert!(
        rendered.contains(&expected_path.display().to_string()),
        "rendered:\n{rendered}"
    );
    assert!(
        rendered.contains("Run `drep init`"),
        "rendered:\n{rendered}"
    );
    assert!(
        rendered.contains("Deterministic checks"),
        "LLM section must not suppress the rest of the report; rendered:\n{rendered}"
    );
}

#[test]
fn two_providers_render_in_file_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "\
        [[llm]]\n\
        model = \"model-a\"\n\
        endpoint = \"http://a.example/v1\"\n\
        \n\
        [[llm]]\n\
        model = \"model-b\"\n\
        endpoint = \"http://b.example/v1\"\n\
        ";
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    let a_idx = rendered
        .find("  1. model-a at http://a.example/v1")
        .expect("first provider line");
    let b_idx = rendered
        .find("  2. model-b at http://b.example/v1")
        .expect("second provider line");
    assert!(
        a_idx < b_idx,
        "providers render in file order; rendered:\n{rendered}"
    );
}

#[test]
fn unset_env_var_is_named_and_provider_still_renders() {
    // The discriminating case: routing display through `config::load` would
    // fail on the unset variable and never print the model. The raw-file
    // path is what makes both halves appear.
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "\
        [[llm]]\n\
        model = \"m\"\n\
        endpoint = \"http://example/v1\"\n\
        api_key = \"${DREP_TEST_DOCTOR_UNSET_XYZ}\"\n\
        ";
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("DREP_TEST_DOCTOR_UNSET_XYZ is NOT set"),
        "rendered:\n{rendered}"
    );
    assert!(
        rendered.contains("  1. m at http://example/v1"),
        "the provider's model/endpoint line must still render; rendered:\n{rendered}"
    );
}

#[test]
fn invalid_toml_is_reported_and_run_to_returns_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "[[llm]\nmodel = "; // truncated mid-key
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to must not error");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("could not be parsed"),
        "rendered:\n{rendered}"
    );
}

#[test]
fn provider_with_no_model_and_no_endpoint_renders_placeholder_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "[[llm]]\n";
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("  1. (no model set) at (no endpoint set)"),
        "rendered:\n{rendered}"
    );
}

#[test]
fn explicit_config_flag_overrides_default_path() {
    // Sanity check for the `--config` path: pointing at a file outside the
    // root makes the report read *that* file rather than `drep.toml`.
    let dir = tempfile::tempdir().expect("tempdir");
    let other = tempfile::tempdir().expect("other tempdir");
    write_py(dir.path());
    std::fs::write(
        other.path().join("custom.toml"),
        "[[llm]]\nmodel = \"x\"\nendpoint = \"http://x/v1\"\n",
    )
    .expect("custom.toml");

    let mut out = Vec::new();
    let exit = run_to(
        &mut out,
        &args_with_config(dir.path(), other.path().join("custom.toml")),
    )
    .expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("  1. x at http://x/v1"),
        "rendered:\n{rendered}"
    );
}
