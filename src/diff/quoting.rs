//! Decoding of the C-style quoting git applies to path names.
//!
//! Git prints a name raw only when every byte is printable ASCII other than
//! `"` and `\` (and, under the default `core.quotePath`, below 0x80). Any
//! other name is wrapped in double quotes, with the byte escaped as one of
//! `\a \b \t \n \v \f \r \" \\` or as three octal digits. Both `--name-only`
//! lines and `---`/`+++` headers use this form.

/// The name `label` spells: decoded when git quoted it, unchanged otherwise.
///
/// `None` for a quoted label that is malformed: unterminated, carrying text
/// after the closing quote, or using an escape git never emits. Decoded bytes
/// that are not UTF-8 are replaced lossily, as the rest of git's output is.
pub(crate) fn unquote(label: &str) -> Option<String> {
    let Some(body) = label.strip_prefix('"') else {
        return Some(label.to_owned());
    };
    let mut bytes = Vec::with_capacity(body.len());
    let mut rest = body.bytes();
    loop {
        match rest.next()? {
            b'"' => break,
            b'\\' => bytes.push(escape(&mut rest)?),
            byte => bytes.push(byte),
        }
    }
    if rest.next().is_some() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The byte one escape sequence stands for, reading just past its `\`.
fn escape(rest: &mut impl Iterator<Item = u8>) -> Option<u8> {
    Some(match rest.next()? {
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        byte @ (b'\\' | b'"') => byte,
        high @ b'0'..=b'3' => {
            let middle = octal_digit(rest.next()?)?;
            let low = octal_digit(rest.next()?)?;
            (high - b'0') * 64 + middle * 8 + low
        }
        _ => return None,
    })
}

fn octal_digit(byte: u8) -> Option<u8> {
    matches!(byte, b'0'..=b'7').then(|| byte - b'0')
}
