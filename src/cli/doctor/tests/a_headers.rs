//! A10: request headers in the LLM section.

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

    let report = super::a_llm_section::report_for(dir.path()).await;

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

    let report = super::a_llm_section::report_for(dir.path()).await;

    assert!(
        report.contains("headers: User-Agent (default)"),
        "got {report}"
    );
}

/// A provider may authenticate entirely through a configured header. Doctor
/// must not prescribe a protocol key as though that working scheme were absent.
#[tokio::test]
async fn header_only_auth_is_not_reported_as_a_missing_credential() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\n\n\
         [llm.headers]\n\"Authorization\" = \"Bearer gateway-secret\"\n",
    )
    .expect("drep.toml");

    let report = super::a_llm_section::report_for(dir.path()).await;

    assert!(
        report.contains("protocol key: not set; configured headers may supply authentication"),
        "got {report}"
    );
    assert!(
        !report.contains("run `drep auth login`"),
        "header authentication must not be diagnosed as missing: {report}"
    );
}
