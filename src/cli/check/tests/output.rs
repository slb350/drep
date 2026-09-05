//! Renderer contracts, with text and JSON subprocess checks for CLI wiring.

use serde_json::{Value, json};

use super::support::{
    outcome, outcome_failing, outcome_with_tool_findings, rendered, rendered_json, run_drep,
};
use crate::analysis::findings::{Finding, Severity};
use crate::analysis::result::FailureReason;
use crate::cli::OutputFormat;
use crate::test_support::{server_returning, write_drep_toml};

async fn run_with_llm(body: &str, args: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let server = server_returning(&[body]).await;
    write_drep_toml(dir.path(), &format!("{}/v1", server.uri()));
    std::fs::write(dir.path().join("lib.py"), "x = 1\n").expect("lib.py");
    run_drep(dir.path(), args)
}

fn finding(line: u32, message: &str, suggestion: Option<&str>) -> Finding {
    Finding {
        kind: "bug".to_owned(),
        severity: Severity::Error,
        file_path: "src/lib.rs".to_owned(),
        line,
        column: None,
        message: message.to_owned(),
        suggestion: suggestion.map(str::to_owned),
        asserts_compile_failure: false,
        fingerprint: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_output_for_a_known_finding_is_exactly_the_expected_string() {
    let output = run_with_llm(
        r#"{"issues":[{"line":1,"severity":"critical","category":"bug","message":"test message","suggestion":"fix it"}]}"#,
        &["check", "lib.py"],
    )
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected Clean exit, stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "lib.py:1: error [llm/bug] test message\n    suggestion: fix it\n    acknowledge: drep acknowledge 60ac6ab2f58ced927ee8b7180d5c9c03498f3aa0fd1107cd36b0302bbbb7899d\n"
    );
}

#[test]
fn clean_run_text_output_is_exactly_no_issues_found() {
    assert_eq!(
        rendered(&outcome(), OutputFormat::Text),
        "No issues found.\n"
    );
}

#[test]
fn json_clean_run_has_unanalyzed_key_present_as_empty_array() {
    let parsed = rendered_json(&outcome());
    assert_eq!(parsed.get("unanalyzed"), Some(&json!([])));
}

#[test]
fn json_findings_distinguish_tool_from_llm_via_source_field() {
    let mut result = outcome_with_tool_findings(vec![finding(1, "tool-msg", None)]);
    result.llm_findings = vec![finding(2, "llm-msg", None)];

    let parsed = rendered_json(&result);
    let findings = parsed["findings"].as_array().expect("findings array");
    let sources: Vec<_> = findings
        .iter()
        .map(|finding| (finding["message"].as_str(), finding["source"].as_str()))
        .collect();
    assert_eq!(
        sources,
        [
            (Some("tool-msg"), Some("tool")),
            (Some("llm-msg"), Some("llm"))
        ]
    );
    assert_eq!(parsed["exit"], 1);
}

#[test]
fn json_uses_the_outcomes_failure_exit() {
    let result = outcome_failing(vec![(
        "bad.py",
        FailureReason::Unreadable("invalid UTF-8".to_owned()),
    )]);
    assert_eq!(rendered_json(&result)["exit"], 2);
}

// A nonblocking finding distinguishes the gate's verdict from a renderer that
// recomputes it as "any finding means exit 1". Keep this through the real CLI.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_exit_matches_the_gate_when_an_llm_finding_does_not_block() {
    let output = run_with_llm(
        r#"{"issues":[{"line":1,"severity":"high","category":"bug","message":"m"}],"summary":""}"#,
        &["check", "--format", "json", "lib.py"],
    )
    .await;
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json parses");

    assert_eq!(
        output.status.code(),
        Some(0),
        "an LLM finding must not block without --fail-on; stderr {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parsed["exit"], 0, "JSON must use the gate's verdict");
    let findings = parsed["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["message"], "m");
    assert_eq!(findings[0]["source"], "llm");
}

#[test]
fn each_suggestion_follows_its_own_finding() {
    let result = outcome_with_tool_findings(vec![
        finding(1, "first", Some("fix one")),
        finding(2, "second", Some("fix two")),
    ]);
    assert_eq!(
        rendered(&result, OutputFormat::Text),
        "src/lib.rs:1: error [tool/bug] first\n    suggestion: fix one\nsrc/lib.rs:2: error [tool/bug] second\n    suggestion: fix two\n"
    );
}

#[test]
fn a_finding_message_is_excerpted_not_printed_raw() {
    let long = "x".repeat(500);
    let result =
        outcome_with_tool_findings(vec![finding(1, &format!("before\u{1b}[2J{long}"), None)]);
    let text = rendered(&result, OutputFormat::Text);

    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "an escape in the message must not reach the terminal: {text:?}"
    );
    let line = text.lines().next().expect("finding line");
    assert!(
        line.chars().count() < 300,
        "the message is bounded, got {} chars",
        line.chars().count()
    );
}
