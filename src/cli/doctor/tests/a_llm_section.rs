//! A5, A6, A7, A8, A9: every shape of the LLM section.

use crate::cli::MachineFiles;
use crate::cli::doctor::{DoctorArgs, run_at};
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

/// Run `doctor` against `dir` and return what it printed.
async fn report_for(dir: &Path) -> String {
    let mut out = Vec::new();
    super::run_scoped(&mut out, &args(dir), dir)
        .await
        .expect("run_to");
    String::from_utf8(out).expect("utf8")
}

#[tokio::test]
async fn no_config_file_prints_the_unconfigured_message_and_still_prints_tools() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());

    let mut out = Vec::new();
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
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

#[tokio::test]
async fn two_providers_render_in_file_order() {
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
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
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

#[tokio::test]
async fn unset_env_var_is_named_and_provider_still_renders() {
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
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
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

#[tokio::test]
async fn invalid_toml_is_reported_and_run_to_returns_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "[[llm]\nmodel = "; // truncated mid-key
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to must not error");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("could not be parsed"),
        "rendered:\n{rendered}"
    );
}

#[tokio::test]
async fn provider_with_no_model_and_no_endpoint_renders_placeholder_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    let body = "[[llm]]\n";
    std::fs::write(dir.path().join("drep.toml"), body).expect("drep.toml");

    let mut out = Vec::new();
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("  1. (no model set) at (no endpoint set)"),
        "rendered:\n{rendered}"
    );
}

#[tokio::test]
async fn explicit_config_flag_overrides_default_path() {
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
    let exit = super::run_scoped(
        &mut out,
        &args_with_config(dir.path(), other.path().join("custom.toml")),
        dir.path(),
    )
    .await
    .expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    assert!(
        rendered.contains("  1. x at http://x/v1"),
        "rendered:\n{rendered}"
    );
}

/// A disabled provider is marked inert rather than listed as if it will run.
///
/// The listing was truthful when only the head was ever consulted only by
/// accident - it said nothing about which entries were live. Now that the list
/// is a real failover chain, an entry the chain will skip is the one thing this
/// section can still misreport.
#[tokio::test]
async fn a_disabled_provider_is_marked_as_skipped() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
enabled = false
model = "local-model"
endpoint = "http://localhost:1234/v1"

[[llm]]
model = "cloud-model"
endpoint = "https://api.example/v1"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    let local = rendered
        .lines()
        .find(|line| line.contains("local-model"))
        .unwrap_or_else(|| panic!("the disabled entry must still be listed:\n{rendered}"));
    assert!(
        local.contains("disabled"),
        "the disabled entry must say so; got {local:?}"
    );
    let cloud = rendered
        .lines()
        .find(|line| line.contains("cloud-model"))
        .expect("the enabled entry is listed");
    assert!(
        !cloud.contains("disabled"),
        "the enabled entry must not be marked skipped; got {cloud:?}"
    );
}

/// With one provider, `doctor` says there is no fallback.
///
/// "Providers are tried in order" is true here and useless. The fact this user
/// needs is that an unreachable endpoint means exit 2 with nothing to catch it.
#[tokio::test]
async fn a_single_provider_is_reported_as_having_no_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        "[[llm]]\nmodel = \"only\"\nendpoint = \"http://localhost:1234/v1\"\n",
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("no fallback"),
        "a lone provider must be reported as having no fallback:\n{rendered}"
    );
}

/// With two enabled providers, `doctor` states the failover rule - including
/// the exception, because "it falls through" without "except on a 401" is the
/// half that leads a user to expect their broken key to be routed around.
#[tokio::test]
async fn two_providers_are_reported_as_a_failover_chain_with_the_401_exception() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
model = "a"
endpoint = "http://a/v1"

[[llm]]
model = "b"
endpoint = "http://b/v1"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("2 providers, tried in order"),
        "the count and the order must be stated:\n{rendered}"
    );
    assert!(
        rendered.contains("401"),
        "the exception must be stated too:\n{rendered}"
    );
}

/// Every provider disabled: the report says the gate cannot run at all.
///
/// `config::load` rejects this file, so `drep check` would fail with the same
/// message - but `doctor` is the command a user runs to find out *why*, and it
/// must not stop at listing two entries that look fine.
#[tokio::test]
async fn an_all_disabled_config_is_reported_as_unable_to_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
enabled = false
model = "a"

[[llm]]
enabled = false
model = "b"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("Every provider is disabled"),
        "an all-disabled config must be called out:\n{rendered}"
    );
}

/// The numbering is the chain position, not the position in the file.
///
/// `drep check` reports a failure as "[1] cloud-model", meaning the head of the
/// chain. If this listing numbered the file instead, a parked entry above would
/// make the same provider "2" here and "1" there - two numbering schemes for
/// one list, and a user counting blocks in `drep.toml` to find the offender
/// would land on the wrong one.
#[tokio::test]
async fn providers_are_numbered_by_chain_position_not_file_position() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
enabled = false
model = "parked"
endpoint = "http://parked/v1"

[[llm]]
model = "leads-the-chain"
endpoint = "http://live/v1"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("1. leads-the-chain at http://live/v1"),
        "the first ENABLED entry is number 1, whatever sits above it:\n{rendered}"
    );
    let parked = rendered
        .lines()
        .find(|line| line.contains("parked"))
        .expect("the disabled entry is still listed");
    assert!(
        !parked.contains("1."),
        "a disabled entry holds no position in the chain; got {parked:?}"
    );
}

/// An `llm` key that is not an array of tables is reported as such.
///
/// Reporting this as "declares no `[[llm]]` provider" points the user at a
/// command that will refuse to overwrite their file and skips the
/// `config::load` check that names the real problem.
#[tokio::test]
async fn a_non_array_llm_key_is_not_reported_as_a_missing_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        "[llm]\nmodel = \"x\"\nendpoint = \"http://a/v1\"\n",
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        !rendered.contains("declares no `[[llm]]` provider"),
        "the key is present - saying it is absent sends the user to `drep init`:\n{rendered}"
    );
    assert!(
        rendered.contains("not a `[[llm]]` array of tables"),
        "the shape problem must be named:\n{rendered}"
    );
    assert!(
        rendered.contains("will not load"),
        "and the load error must still be surfaced:\n{rendered}"
    );
}

/// An empty `llm = []` array reports as "no provider", not "all disabled".
///
/// The two look alike once the entries are gone: an empty array walks the same
/// listing loop as an all-disabled one and reaches the same trailing summary.
/// They are different problems — one needs a provider written, the other needs
/// one re-enabled — and the message has to send the user to the right fix.
#[tokio::test]
async fn an_empty_llm_array_is_reported_as_declaring_no_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("drep.toml"), "llm = []\n").expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("declares no `[[llm]]` provider"),
        "an empty array declares no provider:\n{rendered}"
    );
    assert!(
        !rendered.contains("Every provider is disabled"),
        "there is nothing to re-enable - that message sends the user to the wrong fix:\n{rendered}"
    );
}

/// A parked provider's unset variable is not reported as a problem.
///
/// `config::load` no longer expands a disabled entry, so the variable is not
/// required — and `doctor` warning "LLM analysis will fail until you export it"
/// about a run that will succeed is the same disagreement, in the opposite
/// direction, that its old narrower `${VAR}` scanner produced.
#[tokio::test]
async fn a_disabled_providers_unset_env_var_is_not_warned_about() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
model = "live"
endpoint = "http://live/v1"

[[llm]]
enabled = false
model = "parked"
endpoint = "http://parked/v1"
api_key = "${DREP_DOCTOR_VAR_THAT_IS_NOT_SET}"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        !rendered.contains("DREP_DOCTOR_VAR_THAT_IS_NOT_SET"),
        "the parked provider's variable is not required:\n{rendered}"
    );
    assert!(
        !rendered.contains("will not load"),
        "and the config does load:\n{rendered}"
    );
}

/// An *enabled* provider's unset variable is still reported.
///
/// The discriminating half: a scanner that skipped every provider would pass
/// the test above and go silent on the one warning that matters.
#[tokio::test]
async fn an_enabled_providers_unset_env_var_is_still_warned_about() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("drep.toml"),
        r#"
[[llm]]
model = "live"
endpoint = "http://live/v1"
api_key = "${DREP_DOCTOR_VAR_THAT_IS_NOT_SET}"

[[llm]]
enabled = false
model = "parked"
"#,
    )
    .expect("write config");

    let rendered = report_for(temp.path()).await;
    assert!(
        rendered.contains("DREP_DOCTOR_VAR_THAT_IS_NOT_SET is NOT set"),
        "the live provider's variable must still be flagged:\n{rendered}"
    );
}

/// Run `doctor` against `dir` with an empty auth store, and return its output.
///
/// The store is a temp path so the report never depends on what the developer
/// has stored, and never writes to the real one.
async fn report_with_empty_store(dir: &Path) -> String {
    let mut out = Vec::new();
    // A path that does not exist, deliberately: this fixture describes a machine
    // with no site policy.
    run_at(
        &mut out,
        &args(dir),
        &MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: &dir.join("absent-site.toml"),
        },
    )
    .await
    .expect("run_at");
    String::from_utf8(out).expect("utf8")
}

#[tokio::test]
async fn a_var_reference_is_echoed_so_the_reader_can_see_which_one() {
    // The whole reason doctor reads the raw tree rather than the loaded config:
    // `${VAR}` has to print as itself.
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\napi_key = \"${SOME_TOKEN}\"\n",
    )
    .expect("config");

    let report = report_with_empty_store(dir.path()).await;

    assert!(report.contains("${SOME_TOKEN}"), "got {report}");
    assert!(report.contains("from drep.toml"), "got {report}");
}

#[tokio::test]
async fn a_literal_key_is_never_echoed() {
    // `config::load` accepts a literal `api_key`, and doctor's output is what
    // people paste into bug reports and CI logs. Printing the value verbatim
    // would put a live credential in both.
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\napi_key = \"sk-live-secret-value\"\n",
    )
    .expect("config");

    let report = report_with_empty_store(dir.path()).await;

    assert!(
        !report.contains("sk-live-secret-value"),
        "the key reached the report: {report}"
    );
    assert!(
        report.contains("a literal value"),
        "and the reader is told a key is set, and to prefer a variable: {report}"
    );
}

#[tokio::test]
async fn a_provider_with_no_key_anywhere_says_where_to_get_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_py(dir.path());
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\n",
    )
    .expect("config");

    let report = report_with_empty_store(dir.path()).await;

    assert!(report.contains("drep auth login"), "got {report}");
}

/// `doctor` says which headers will actually be sent, and never what they hold.
///
/// The effective set, through the same `config::effective_headers` the client
/// uses, so the report cannot describe a set the request will not carry. Values
/// are withheld because this output gets pasted into issues and chat.
#[tokio::test]
async fn configured_header_names_are_listed_without_their_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\napi_key = \"k\"\n\n\
         [llm.headers]\n\"X-Tenant-Token\" = \"super-secret-value\"\n\"User-Agent\" = \"acme/1.0\"\n",
    )
    .expect("drep.toml");

    let report = report_for(dir.path()).await;

    assert!(
        report.contains("headers: User-Agent, X-Tenant-Token"),
        "names, sorted: {report}"
    );
    assert!(
        !report.contains("super-secret-value"),
        "a header value must never reach the report: {report}"
    );
}

/// An entry configuring no headers still reports the one drep sends.
///
/// The case the listing used to be silent about, and the one where silence
/// mattered: the operator debugging a gateway 403 is asking what user agent is
/// going out precisely because they never set one.
#[tokio::test]
async fn the_default_user_agent_is_reported_even_when_none_is_configured() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\napi_key = \"k\"\n",
    )
    .expect("drep.toml");

    let report = report_for(dir.path()).await;

    assert!(
        report.contains("headers: User-Agent (default)"),
        "got {report}"
    );
}
