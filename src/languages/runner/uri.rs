//! Turn a `file:` URI into the path drep was asked to check.
//!
//! Split from `parsers.rs` because it is a URI decoder rather than a diagnostic
//! reader: SARIF is the only format that mandates a URI, but the decoding is
//! the spec's rather than checkstyle's, so it belongs beside the other
//! encoding rules rather than among the per-tool parsers.
//!
//! Decoding is complete before the path leaves here. `check` looks a finding's
//! absolute path up in a table to rewrite it back to the path the user typed,
//! and a half-decoded URI matches nothing in that table, so the finding keeps a
//! location no file has.

/// `file:/abs/path` and `file:///abs/path` both name a local path. drep matches
/// findings against the paths it was asked to check, so a finding left under a
/// URI is filed against a path that matches nothing and is silently dropped.
///
/// The path is percent-decoded because producers encode it: checkstyle's
/// SarifLogger maps a space to `%20` and a quote to `%22`, and a spec-compliant
/// producer percent-encodes everything reserved. Left encoded, the finding's
/// path never matches the file drep was asked to check.
pub(super) fn strip_file_uri(uri: &str) -> String {
    let path = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .unwrap_or(uri);
    // `file:///abs` leaves a leading empty authority; `file:/abs` does not.
    if path.is_empty() {
        uri.to_owned()
    } else {
        strip_drive_root(percent_decode(path))
    }
}

/// `file:/C:/repo/Sample.java` is how a Windows producer names a local path.
/// RFC 8089 requires the path component to begin with `/`, so the drive letter
/// arrives one slash deeper than the path it denotes. Left in place that slash
/// makes the path drive-relative rather than absolute, so it matches nothing in
/// the absolute-path table `check` uses to rewrite a finding back to the path
/// the user asked about, and every Windows checkstyle finding keeps a location
/// no file has.
///
/// Applied on every platform rather than under `cfg(windows)`: a `cfg`-gated
/// twin is invisible to the mutation gate, and a first component that is a bare
/// ASCII letter followed by a colon is not a path any Unix producer emits.
///
/// Four named conditions rather than one slice pattern, for the reason
/// `percent_decode` writes `hi * 16 + lo` below: cargo-mutants does not mutate
/// match patterns, so `matches!(path.as_bytes(), [b'/', d, b':', ..])` states
/// the same rule while making every part of it invisible to the gate. Written
/// this way it carries five mutable operators, and the fixture table in
/// `sarif_parser_keeps_paths_that_only_look_like_a_drive` has one row per
/// operator to discriminate them.
fn strip_drive_root(mut path: String) -> String {
    let mut chars = path.chars();
    let rooted = chars.next() == Some('/');
    let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let colon = chars.next() == Some(':');
    // A drive prefix is the whole path or is followed by a separator. `/C:x`
    // is an ordinary relative-looking name, not a drive.
    let bounded = matches!(chars.next(), None | Some('/'));
    if rooted && drive && colon && bounded {
        path.remove(0);
    }
    path
}

/// Decode RFC 3986 `%HH` sequences byte-wise. Anything that is not a valid
/// triplet passes through verbatim, and malformed UTF-8 at the end of decoding
/// degrades lossily rather than failing the finding. checkstyle encodes only
/// the space and the quote, so a literal `%20` *in* a filename is ambiguous
/// with an encoded one at the source; decoding is the better wrong there,
/// because spaces in paths are common and `%20` in a name is not.
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let triplet = match bytes.get(i..i + 3) {
            // `hi * 16 + lo`, not `hi << 4 | lo`: the two nibbles never share
            // a set bit, so `|` and `^` agree on every reachable input and the
            // mutation gate cannot observe the difference.
            Some(&[b'%', hi, lo]) => hex_value(hi)
                .zip(hex_value(lo))
                .map(|(hi, lo)| hi * 16 + lo),
            _ => None,
        };
        match triplet {
            Some(byte) => {
                decoded.push(byte);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|d| d as u8)
}
