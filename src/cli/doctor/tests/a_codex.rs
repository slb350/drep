//! Codex-specific doctor readiness and redaction contracts.

use std::path::Path;

use crate::llm::codex::CodexStatus;
use crate::test_support::write_site_policy;

async fn report_with_probe(dir: &Path, probe: &dyn Fn() -> Result<CodexStatus, String>) -> String {
    report_with_probe_and_policy(dir, &dir.join("absent-site.toml"), probe).await
}

async fn report_with_probe_and_policy(
    dir: &Path,
    policy: &Path,
    probe: &dyn Fn() -> Result<CodexStatus, String>,
) -> String {
    super::report_scoped_with_codex(dir, policy, probe).await
}

#[tokio::test]
async fn a_refused_repository_does_not_probe_codex_readiness() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("source");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nbackend = \"codex\"\nmodel = \"gpt-5.6-sol\"\n",
    )
    .expect("config");
    let policy = write_site_policy(dir.path(), &[".drep-no-llm"]);
    std::fs::write(dir.path().join(".drep-no-llm"), "").expect("marker");
    let calls = std::cell::Cell::new(0);

    let report = report_with_probe_and_policy(dir.path(), &policy, &|| {
        calls.set(calls.get() + 1);
        Ok(CodexStatus::new("unused"))
    })
    .await;

    assert_eq!(calls.get(), 0, "doctor started Codex for a refused review");
    assert!(
        report.contains("not attempted") && report.contains("site policy"),
        "the skipped readiness probe needs an explicit reason: {report}"
    );
}

#[tokio::test]
async fn a_ready_codex_backend_reports_only_redacted_subscription_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("source");
    std::fs::write(
        dir.path().join("drep.toml"),
        r#"
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
"#,
    )
    .expect("config");

    let report = report_with_probe(dir.path(), &|| Ok(CodexStatus::new("0.148.0"))).await;

    assert!(
        report.contains("gpt-5.6-sol via ChatGPT/Codex subscription"),
        "got {report}"
    );
    assert!(report.contains("Codex CLI: 0.148.0"), "got {report}");
    assert!(
        report.contains("authentication: ChatGPT-managed"),
        "got {report}"
    );
    assert!(
        report.contains("isolation: ephemeral, read-only, tools disabled"),
        "got {report}"
    );
    assert!(!report.contains("no endpoint"), "got {report}");
    assert!(!report.contains("key:"), "got {report}");
}

#[tokio::test]
async fn a_codex_readiness_failure_is_actionable_but_not_a_doctor_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        r#"
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
"#,
    )
    .expect("config");

    let report = report_with_probe(dir.path(), &|| {
        Err("Codex CLI was not found; install it and run `codex login`".to_owned())
    })
    .await;

    assert!(report.contains("Codex CLI was not found"), "got {report}");
    assert!(report.contains("`codex login`"), "got {report}");
}

#[tokio::test]
async fn a_disabled_codex_backend_does_not_run_the_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        r#"
[[llm]]
backend = "codex"
enabled = false
model = "gpt-5.6-sol"
reasoning_effort = "high"

[[llm]]
model = "local"
endpoint = "http://localhost:1234/v1"
"#,
    )
    .expect("config");
    let calls = std::cell::Cell::new(0);

    let report = report_with_probe(dir.path(), &|| {
        calls.set(calls.get() + 1);
        Ok(CodexStatus::new("unused"))
    })
    .await;

    assert_eq!(calls.get(), 0, "disabled providers are inert");
    assert!(
        report.contains("via ChatGPT/Codex subscription (disabled - skipped)"),
        "got {report}"
    );
}

#[tokio::test]
async fn a_codex_only_config_never_reads_the_http_auth_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        r#"
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
"#,
    )
    .expect("config");
    std::fs::write(dir.path().join("auth.toml"), "not valid TOML = [")
        .expect("corrupt HTTP auth store");

    let report = report_with_probe(dir.path(), &|| Ok(CodexStatus::new("0.148.0"))).await;

    assert!(
        !report.contains("auth store could not be read"),
        "a Codex-only setup must not consult HTTP credentials: {report}"
    );
}
