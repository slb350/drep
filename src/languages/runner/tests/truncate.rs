//! `truncate`, which trims a tool's other-stream output for the `detail` field.
//!
//! Every case here exists to make a specific mutation observable. The function
//! is four lines and looks obviously correct, which is exactly why it shipped
//! with no tests at all and six surviving mutants — including one that returned
//! the *untruncated* string and one that panicked on multi-byte input.
//!
//! The multi-byte cases carry the weight. An ASCII-only test cannot distinguish
//! the boundary walk from no boundary walk, because every ASCII index is
//! already a char boundary.

use crate::languages::runner::truncate;

#[test]
fn short_input_is_returned_unchanged() {
    assert_eq!(truncate("hello", 10), "hello");
}

#[test]
fn input_exactly_at_the_limit_is_unchanged() {
    assert_eq!(truncate("hello", 5), "hello");
}

#[test]
fn long_input_is_actually_truncated() {
    // Pins the early return's comparison. Inverting `<=` to `>` returns the
    // whole string here, which is the opposite of the function's purpose.
    assert_eq!(truncate("abcdefghij", 3), "abc");
}

#[test]
fn empty_input_is_empty() {
    assert_eq!(truncate("", 5), "");
}

#[test]
fn zero_limit_yields_empty_string() {
    assert_eq!(truncate("abc", 0), "");
}

#[test]
fn a_cut_landing_mid_character_backs_off_to_a_boundary() {
    // "héllo" is h(1) é(2 bytes, at 1..3) l l o. Byte 2 is inside `é`, so
    // slicing there would panic. The walk must step back to 1.
    //
    // Catches: `>` -> `==` and the deleted `!`, both of which exit the loop
    // immediately and slice mid-character; and `-=` -> `+=`, which walks
    // forward to 3 and returns "hé" — longer than the limit it was given.
    assert_eq!(truncate("héllo", 2), "h");
}

#[test]
fn backing_off_walks_one_byte_at_a_time() {
    // "ab¢de" is a(0) b(1) ¢(2..4) d(4) e(5); byte 3 is inside `¢`, and 2 is a
    // boundary. Decrementing gives "ab"; halving the index gives "a".
    //
    // This is the only case that distinguishes `-= 1` from `/= 2`, since with a
    // limit of 2 both land on the same index.
    assert_eq!(truncate("ab¢de", 3), "ab");
}

#[test]
fn a_string_that_is_entirely_one_multibyte_character_truncates_to_empty() {
    // Catches `end > 0` -> `end == 0`: the walk must terminate at 0 rather than
    // run past it, and slicing at 0 is always valid.
    assert_eq!(truncate("é", 1), "");
}

#[test]
fn never_returns_more_bytes_than_the_limit() {
    // The property the individual cases are sampling. Any forward-walking
    // mutation violates it for some input.
    for limit in 0..12 {
        for input in ["", "abc", "héllo", "ab¢de", "日本語text", "abcdefghij"] {
            let out = truncate(input, limit);
            assert!(
                out.len() <= limit,
                "truncate({input:?}, {limit}) returned {out:?}, longer than the limit"
            );
            assert!(input.starts_with(&out), "output must be a prefix of input");
        }
    }
}
