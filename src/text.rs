//! Text that comes from outside drep and lands in a terminal.

/// A one-line, bounded, control-character-free excerpt of untrusted text.
///
/// Bounded because a reasoning model can return kilobytes, and a URL in a
/// markdown file can be a multi-kilobyte data: URI; both end up in a terminal
/// and in `--format json`. Control characters are replaced rather than passed
/// through: the text is not drep's, and an escape sequence in it would
/// otherwise be interpreted by the terminal reading the report.
///
/// Shared rather than per-caller. The second copy, added for `bare_url`
/// messages, truncated but did not strip control characters - the one thing
/// this function exists for.
pub fn excerpt(body: &str, max_chars: usize) -> String {
    // Trimmed before cleaning rather than after, and the kept length is
    // carried rather than recounted. This runs once per rendered finding, and
    // the previous form built a full cleaned copy of the whole body - of which
    // `max_chars` survive - then called `out.chars().count()` on every
    // iteration, rescanning everything kept so far.
    //
    // Trimming first is equivalent because cleaning only ever maps a control
    // character to a space: an edge character that `trim` would have removed
    // after cleaning is whitespace or a control character before it, and an
    // interior one still becomes a space below.
    let trimmed = body.trim_matches(|c: char| c.is_whitespace() || c.is_control());
    let mut out = String::with_capacity(max_chars);
    let mut kept = 0usize;
    let mut last_was_space = false;
    for c in trimmed
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
    {
        if kept >= max_chars {
            out.push('…');
            break;
        }
        if c == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        out.push(c);
        kept += 1;
    }
    if out.is_empty() {
        // Unreachable in practice for a model response - an empty body is a
        // transport failure long before this - but a quoted empty string reads
        // as a bug in drep.
        return "<nothing>".to_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn control_characters_are_replaced_not_passed_through() {
        // The whole point: an escape sequence reaching a terminal is the
        // failure this guards against.
        let out = excerpt("a\u{1b}[31mred\u{7}b", 100);
        assert!(!out.chars().any(char::is_control), "{out:?}");
        assert!(out.contains("red"), "{out:?}");
    }

    #[test]
    fn text_within_the_limit_is_returned_intact_without_a_marker() {
        assert_eq!(excerpt("short", 100), "short");
    }

    #[test]
    fn text_over_the_limit_is_cut_and_marked() {
        let out = excerpt(&"x".repeat(50), 10);
        assert_eq!(out.chars().count(), 11, "{out:?}");
        assert!(out.ends_with('…'), "{out:?}");
    }

    #[test]
    fn runs_of_space_collapse_and_the_edges_are_trimmed() {
        assert_eq!(excerpt("  a\t\t\tb  ", 100), "a b");
    }

    #[test]
    fn empty_input_names_itself_rather_than_quoting_nothing() {
        assert_eq!(excerpt("   ", 100), "<nothing>");
    }

    #[test]
    fn the_limit_counts_characters_not_bytes() {
        // Ten em dashes is thirty bytes. A byte-counting limit would cut this
        // to three characters, and could cut mid-codepoint.
        let out = excerpt(&"—".repeat(10), 10);
        assert_eq!(out.chars().count(), 10, "{out:?}");
        assert!(!out.ends_with('…'), "{out:?}");
    }
}
