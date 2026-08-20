//! The Codex invocation is an isolation contract, not merely a command line.

use std::ffi::OsString;
use std::path::Path;

use crate::analysis::response_contract::output_schema;
use crate::config::ReasoningEffort;
use crate::llm::codex::command::{
    ChildEnvironment, instructions_text, invocation_args, sensitive_name, toml_override,
};

#[test]
fn every_argument_is_present_in_the_load_bearing_order() {
    let args = invocation_args(
        "gpt-5.6-sol",
        Some(&ReasoningEffort::High),
        Path::new("/tmp/drep codex/instructions.md"),
        Path::new("/tmp/drep codex/schema.json"),
        Path::new("/tmp/drep codex/cwd"),
    )
    .expect("UTF-8 fixture paths");
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.into_string().expect("UTF-8 fixture"))
        .collect();

    assert_eq!(
        args,
        vec![
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "model_instructions_file=\"/tmp/drep codex/instructions.md\"",
            "-c",
            "model_reasoning_effort=\"high\"",
            "-c",
            "project_doc_max_bytes=0",
            "-c",
            "web_search=\"disabled\"",
            "--disable",
            "shell_tool",
            "--disable",
            "unified_exec",
            "--disable",
            "apps",
            "--disable",
            "multi_agent",
            "--disable",
            "hooks",
            "--disable",
            "memories",
            "-a",
            "never",
            "exec",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "-C",
            "/tmp/drep codex/cwd",
            "--model",
            "gpt-5.6-sol",
            "--output-schema",
            "/tmp/drep codex/schema.json",
            "--json",
            "-",
        ]
    );
}

#[test]
fn absent_reasoning_effort_leaves_the_cli_default_unset() {
    let args = invocation_args(
        "gpt-5.6-sol",
        None,
        Path::new("/tmp/instructions.md"),
        Path::new("/tmp/schema.json"),
        Path::new("/tmp/cwd"),
    )
    .expect("paths");
    let args = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>();

    assert!(
        !args
            .iter()
            .any(|arg| arg.contains("model_reasoning_effort")),
        "the CLI default must not be replaced: {args:?}"
    );
}

#[test]
fn private_files_replace_generic_instructions_and_pin_the_response_shape() {
    let instructions = instructions_text("Review Rust carefully.");
    assert!(instructions.starts_with("Review Rust carefully."));
    assert!(instructions.contains("Use only the code supplied on stdin"));
    assert!(instructions.contains("Do not use tools"));

    let schema = output_schema();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], serde_json::json!(["issues", "summary"]));
    let issue = &schema["properties"]["issues"]["items"];
    assert_eq!(issue["additionalProperties"], false);
    assert_eq!(
        issue["required"],
        serde_json::json!([
            "line",
            "severity",
            "category",
            "message",
            "suggestion",
            "code_snippet"
        ])
    );
    assert_eq!(
        issue["properties"]["severity"]["enum"],
        serde_json::json!(["critical", "high", "medium", "low", "info"])
    );
}

#[test]
fn toml_overrides_encode_quotes_and_backslashes_with_toml_itself() {
    let path = r#"C:\reviewer's \"prompt\".md"#;
    let encoded = toml_override("model_instructions_file", path);
    let decoded: toml::Value = toml::from_str(&encoded).expect("override is valid TOML");
    assert_eq!(decoded["model_instructions_file"].as_str(), Some(path));
}

#[test]
fn child_environment_is_an_allowlist_and_never_forwards_api_keys() {
    let environment = ChildEnvironment::from_iter(
        [
            ("PATH", "/bin"),
            ("HOME", "/home/test"),
            ("CODEX_HOME", "/home/test/.codex"),
            ("HTTPS_PROXY", "http://proxy"),
            ("SSL_CERT_FILE", "/certs/ca.pem"),
            ("LANG", "en_US.UTF-8"),
            ("OPENAI_API_KEY", "openai-secret"),
            ("KIMI_API_KEY", "kimi-secret"),
            ("DREP_AUTH_PATH", "/real/auth.toml"),
            ("UNRELATED_SECRET", "secret"),
        ]
        .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );

    assert_eq!(environment.get("PATH"), Some("/bin"));
    assert_eq!(environment.get("HOME"), Some("/home/test"));
    assert_eq!(environment.get("CODEX_HOME"), Some("/home/test/.codex"));
    assert_eq!(environment.get("HTTPS_PROXY"), Some("http://proxy"));
    assert_eq!(environment.get("SSL_CERT_FILE"), Some("/certs/ca.pem"));
    assert_eq!(environment.get("LANG"), Some("en_US.UTF-8"));
    for forbidden in [
        "OPENAI_API_KEY",
        "KIMI_API_KEY",
        "DREP_AUTH_PATH",
        "UNRELATED_SECRET",
    ] {
        assert_eq!(environment.get(forbidden), None, "forwarded {forbidden}");
    }
}

#[test]
fn each_secret_name_pattern_is_independently_forbidden() {
    assert!(sensitive_name("DREP_ANYTHING"));
    assert!(sensitive_name("VENDOR_API_KEY"));
    assert!(!sensitive_name("PATH"));
}
