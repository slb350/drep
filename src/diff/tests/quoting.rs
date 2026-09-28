//! Decoding a path field the way git's `unquote_c_style` does.

use std::path::PathBuf;

use crate::diff::quoting::decode;

#[test]
fn a_field_git_did_not_quote_is_the_path_itself() {
    assert_eq!(decode("src/a b.rs"), PathBuf::from("src/a b.rs"));
    // Only a leading quote opens a quoted field.
    assert_eq!(decode("say\"hi\".rs"), PathBuf::from("say\"hi\".rs"));
}

#[test]
fn every_named_escape_decodes_to_its_byte() {
    for (escape, byte) in [
        ('a', 0x07),
        ('b', 0x08),
        ('t', b'\t'),
        ('n', b'\n'),
        ('v', 0x0b),
        ('f', 0x0c),
        ('r', b'\r'),
        ('\\', b'\\'),
        ('"', b'"'),
    ] {
        let decoded = decode(&format!("\"x\\{escape}y\""));
        let expected = format!("x{}y", char::from(byte));
        assert_eq!(decoded, PathBuf::from(&expected), "escape \\{escape}");
    }
}

/// Three octal digits are one byte. `\303\251` is UTF-8 `é`; each digit
/// position carries a non-zero digit somewhere below, so every weight in the
/// arithmetic is observable.
#[test]
fn octal_escapes_decode_to_their_bytes() {
    assert_eq!(
        decode("\"caf\\303\\251.rs\""),
        PathBuf::from("caf\u{e9}.rs")
    );
    assert_eq!(decode("\"\\177\\001\""), PathBuf::from("\u{7f}\u{1}"));
    assert_eq!(decode("\"\\067\""), PathBuf::from("7"));
}

/// Git never prints these, so they are left exactly as they arrived rather
/// than half-decoded into a path nobody has.
#[test]
fn a_field_git_would_not_write_is_returned_verbatim() {
    for field in [
        "\"a\\xb\"",
        "\"\\400\"",
        "\"\\38x\"",
        "\"\\3x0\"",
        "\"\\30\"",
        "\"trailing\\\"",
        "\"a\"b\"",
        "\"unterminated",
        "\"",
    ] {
        assert_eq!(decode(field), PathBuf::from(field), "field {field}");
    }
}

/// A name that is not UTF-8 keeps its exact bytes rather than a replacement
/// character that names no file on disk.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_keeps_its_exact_bytes() {
    use std::os::unix::ffi::OsStrExt;

    let decoded = decode("\"caf\\351.rs\"");
    assert_eq!(decoded.as_os_str().as_bytes(), b"caf\xe9.rs");
}
