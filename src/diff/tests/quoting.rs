//! `quoting::unquote`: git's C-style path quoting.

use crate::diff::quoting::unquote;

#[test]
fn an_unquoted_label_is_returned_unchanged() {
    assert_eq!(unquote("b/src/lib.rs").as_deref(), Some("b/src/lib.rs"));
    assert_eq!(unquote("").as_deref(), Some(""));
}

#[test]
fn a_quoted_label_loses_its_quotes() {
    assert_eq!(unquote("\"b/my file.rs\"").as_deref(), Some("b/my file.rs"));
}

#[test]
fn every_named_escape_decodes_to_its_byte() {
    assert_eq!(
        unquote(r#""\a\b\t\n\v\f\r\"\\""#).as_deref(),
        Some("\u{7}\u{8}\t\n\u{b}\u{c}\r\"\\")
    );
}

#[test]
fn octal_escapes_weigh_each_digit_by_its_place() {
    // 0o123 = 83 ('S'), 0o044 = 36 ('$'), 0o177 = DEL: each digit position
    // is distinct and nonzero somewhere, so a wrong weight changes a byte.
    assert_eq!(unquote(r#""\123\044\177""#).as_deref(), Some("S$\u{7f}"));
}

#[test]
fn octal_escapes_rebuild_multibyte_utf8() {
    assert_eq!(
        unquote(r#""b/\303\274n\303\257.rs""#).as_deref(),
        Some("b/ünï.rs")
    );
}

#[test]
fn bytes_that_are_not_utf8_are_replaced() {
    assert_eq!(unquote(r#""b/\377.rs""#).as_deref(), Some("b/\u{fffd}.rs"));
}

#[test]
fn malformed_quoted_labels_are_rejected() {
    for label in [
        r#""unterminated"#,
        r#""ends in an escape\"#,
        r#""closed" then more"#,
        r#""\x41""#,
        r#""\400""#,
        r#""\18""#,
        r#""\108""#,
        r#""\1""#,
        r#""\12""#,
    ] {
        assert_eq!(unquote(label), None, "{label}");
    }
}
