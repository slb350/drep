//! [`display_endpoint`]: an endpoint as drep prints it.

use super::super::*;

#[test]
fn an_endpoint_without_a_credential_is_returned_as_written() {
    // Untouched, not reserialised: the parser would lowercase the host and add
    // a trailing slash, and a report should echo what the user wrote.
    let endpoint = "https://API.Example/v1";

    let shown = display_endpoint(endpoint);

    assert!(matches!(shown, Cow::Borrowed(_)), "{shown:?}");
    assert_eq!(shown, endpoint);
}

#[test]
fn a_user_and_password_are_replaced_by_the_marker() {
    assert_eq!(
        display_endpoint("https://drep:gateway-secret@gateway.example:8443/v1"),
        "https://<redacted>@gateway.example:8443/v1"
    );
}

#[test]
fn a_username_alone_is_a_credential() {
    // `https://token@host` is how a token-only gateway is configured; reqwest
    // sends it as Basic auth with an empty password.
    assert_eq!(
        display_endpoint("https://sk-token@gateway.example/v1"),
        "https://<redacted>@gateway.example/v1"
    );
}

#[test]
fn a_password_alone_is_a_credential() {
    assert_eq!(
        display_endpoint("http://:only-secret@127.0.0.1:1234/v1"),
        "http://<redacted>@127.0.0.1:1234/v1"
    );
}

#[test]
fn an_at_sign_after_a_backslash_is_not_mistaken_for_a_credential() {
    // For http(s) the URL parser reads `\` as `/`, so this names the host
    // `evil.example` and carries no credential. Cutting at the `@` would print
    // `host` as the host instead of the one a request actually goes to.
    let endpoint = "http://evil.example\\@host/v1";

    let parsed = reqwest::Url::parse(endpoint).expect("parses");
    assert_eq!(parsed.host_str(), Some("evil.example"));
    assert_eq!(parsed.username(), "");
    assert_eq!(display_endpoint(endpoint), endpoint);
}

#[test]
fn a_file_url_with_a_credential_is_redacted_whole() {
    // A `file:` URL cannot hold one, so the parser refuses it; the text is
    // still a password.
    assert_eq!(display_endpoint("file://drep:secret@host/x"), "<redacted>");
}

#[test]
fn an_at_sign_in_the_path_is_left_alone() {
    let endpoint = "https://gateway.example/@team/v1";

    assert_eq!(display_endpoint(endpoint), endpoint);
}

#[test]
fn an_unparseable_endpoint_with_an_at_sign_is_redacted_whole() {
    // The port is not a number, so no request ever goes out - but the text is
    // still what the user typed their password into.
    assert_eq!(
        display_endpoint("https://drep:gateway-secret@gateway.example:port/v1"),
        "<redacted>"
    );
}

#[test]
fn an_unparseable_endpoint_without_one_is_returned_as_written() {
    let endpoint = "not a url";

    assert_eq!(display_endpoint(endpoint), endpoint);
}

#[test]
fn a_scheme_less_endpoint_with_a_credential_is_redacted_whole() {
    // The parser reads `user` as the scheme and the rest as an opaque path,
    // so there is no username to strip, but the password is right there.
    assert_eq!(
        display_endpoint("drep:gateway-secret@localhost:1234/v1"),
        "<redacted>"
    );
}

#[test]
fn a_scheme_less_endpoint_without_one_is_returned_as_written() {
    let endpoint = "localhost:11434/v1";

    assert_eq!(display_endpoint(endpoint), endpoint);
}
