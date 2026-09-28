//! Paths as git prints them.
//!
//! Git prints a path verbatim unless it holds a byte its quoting table escapes:
//! a control character, `"` or `\` always, and every byte above 0x7f while
//! `core.quotePath` is on. It then prints the whole field in double quotes with
//! C-style escapes, prefix included: `"b/src/caf\303\251.rs"`. `--name-only`
//! lines and patch headers use the same form, so this is the one decoder for
//! both, and a path leaves the diff module decoded.
//!
//! The rules are those of `unquote_c_style` in git's `quote.c`, the inverse of
//! the `quote_c_style` that wrote the field.

use std::path::PathBuf;

/// The path a field of git's output names.
///
/// A field git did not quote is the path itself. A quoted field that does not
/// decode under git's rules is returned verbatim, matching the parser's
/// tolerance: git never writes one, and a half-decoded path would name a file
/// that does not exist.
pub(super) fn decode(field: &str) -> PathBuf {
    match unquote(field) {
        Some(bytes) => path_from_bytes(bytes),
        None => PathBuf::from(field),
    }
}

/// The bytes a whole quoted field encodes, or `None` if `field` is not exactly
/// one well-formed quoted string.
fn unquote(field: &str) -> Option<Vec<u8>> {
    let body = field.strip_prefix('"')?.strip_suffix('"')?;
    let mut bytes = body.bytes();
    let mut decoded = Vec::with_capacity(body.len());
    while let Some(byte) = bytes.next() {
        match byte {
            // Unescaped, a quote ends the string, and this one is not last.
            b'"' => return None,
            b'\\' => decoded.push(escaped(&mut bytes)?),
            other => decoded.push(other),
        }
    }
    Some(decoded)
}

/// The byte an escape sequence stands for, reading what follows its backslash.
fn escaped(bytes: &mut impl Iterator<Item = u8>) -> Option<u8> {
    let byte = match bytes.next()? {
        b'a' => 0x07,
        b'b' => 0x08,
        b't' => b'\t',
        b'n' => b'\n',
        b'v' => 0x0b,
        b'f' => 0x0c,
        b'r' => b'\r',
        verbatim @ (b'\\' | b'"') => verbatim,
        // Three octal digits. A leading digit above 3 would overflow a byte,
        // and git rejects it. `high * 64 + middle * 8 + low` rather than shifts
        // and `|` for the reason `percent_decode` gives: the digits never share
        // a set bit, so `|` and `+` would be indistinguishable to the mutation
        // gate.
        high @ b'0'..=b'3' => {
            let middle = octal_digit(bytes.next()?)?;
            let low = octal_digit(bytes.next()?)?;
            (high - b'0') * 64 + middle * 8 + low
        }
        _ => return None,
    };
    Some(byte)
}

fn octal_digit(byte: u8) -> Option<u8> {
    (byte as char).to_digit(8).map(|digit| digit as u8)
}

/// A path from decoded bytes, which need not be UTF-8.
///
/// Every release target is Unix, where a path is bytes and the name round-trips
/// exactly. Elsewhere git stores UTF-8 names, so the lossy conversion never
/// replaces anything. The `cfg` covers only the differing expression; see
/// `languages::runner::is_executable` for why there is no gated twin.
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes)
    };
    #[cfg(not(unix))]
    let path = String::from_utf8_lossy(&bytes).into_owned();
    PathBuf::from(path)
}
