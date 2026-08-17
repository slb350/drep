//! `payload::render`: criteria 19-26.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::analysis::payload::render;
use crate::diff::hunks::{Hunk, HunkLine};
use crate::languages;
use crate::languages::spec::LanguageSupport;

/// The Rust entry from the registry.
///
/// `render` takes a `LanguageSupport` rather than a name, so these tests
/// cannot pass a string that drifts from what the registry would say. The
/// payload header therefore reads `Rust` (the `display_name`), never `rust`
/// (the registry key).
fn rust() -> &'static LanguageSupport {
    languages::detect(Path::new("probe.rs")).expect("rust is a registered language")
}

fn context(text: &str) -> HunkLine {
    HunkLine::Context(text.to_owned())
}

fn added(text: &str) -> HunkLine {
    HunkLine::Added(text.to_owned())
}

fn removed(text: &str) -> HunkLine {
    HunkLine::Removed(text.to_owned())
}

#[test]
fn empty_hunk_slice_returns_none() {
    assert!(render(rust(), &[]).is_none());
}

#[test]
fn a_known_hunk_matches_the_exact_payload_text() {
    // The format contract. Asserting the whole string — not a contains —
    // because a `contains` would let any text that happens to mention
    // the right tokens pass, even if the gutter or the scope sentence
    // were subtly wrong.
    let hunk = Hunk {
        file_path: PathBuf::from("src/lib.rs"),
        old_start: 11,
        old_count: 4,
        new_start: 12,
        new_count: 5,
        lines: vec![
            context("fn compute(xs: &[u32]) -> u32 {"),
            context("    let mut total = 0;"),
            removed("    let total = xs.iter().sum();"),
            added("    for x in xs {"),
            added("        total += x;"),
            added("    }"),
            context("    total"),
            context("}"),
        ],
    };

    let payload = render(rust(), std::slice::from_ref(&hunk)).expect("render");

    let expected = "\
File: src/lib.rs
Language: Rust

Review the lines marked `+`. Lines with no marker are unchanged context. Lines marked `-` were removed and have no line number; do not report findings on them. Report each finding using the line number shown in the gutter.

     12 | fn compute(xs: &[u32]) -> u32 {
     13 |     let mut total = 0;
-       |     let total = xs.iter().sum();
+    14 |     for x in xs {
+    15 |         total += x;
+    16 |     }
     17 |     total
     18 | }
";
    assert_eq!(payload.text, expected);
}

#[test]
fn removed_line_renders_with_six_spaces_and_does_not_consume_a_number() {
    let hunk = Hunk {
        file_path: PathBuf::from("counter.rs"),
        old_start: 9,
        old_count: 4,
        new_start: 10,
        new_count: 3,
        lines: vec![context("before"), removed("deleted"), context("after")],
    };

    let payload = render(rust(), std::slice::from_ref(&hunk)).expect("render");

    assert!(
        payload.text.contains("-       | deleted"),
        "the removed line must use six spaces where the number goes, got:\n{}",
        payload.text
    );
    assert!(
        payload.text.contains("     11 | after"),
        "the Context after the Removed line must carry the number the Removed line did not consume, got:\n{}",
        payload.text
    );
}

#[test]
fn valid_lines_uses_file_line_numbers_not_payload_relative_indices() {
    let hunk = Hunk {
        file_path: PathBuf::from("anchored.rs"),
        old_start: 199,
        old_count: 1,
        new_start: 200,
        new_count: 1,
        lines: vec![context("hi")],
    };

    let payload = render(rust(), std::slice::from_ref(&hunk)).expect("render");

    assert!(
        payload.valid_lines.contains(&200),
        "expected 200 in valid_lines, got {:?}",
        payload.valid_lines
    );
    assert!(
        !payload.valid_lines.contains(&1),
        "1 must not appear in valid_lines (payload-relative numbering), got {:?}",
        payload.valid_lines
    );
}

#[test]
fn valid_lines_contains_context_and_added_numbers_only() {
    let hunk = Hunk {
        file_path: PathBuf::from("mix.rs"),
        old_start: 9,
        old_count: 4,
        new_start: 10,
        new_count: 3,
        // The trailing `removed` is load-bearing. A removed line in the
        // *middle* of a hunk shares `current_number` with the numbered line
        // that follows it, so an implementation that wrongly inserted a
        // number for removed lines would be indistinguishable from the
        // correct one - the number lands in the set either way. Only a
        // removed line with no numbered line after it exposes the bug, by
        // contributing a number that is one past the end of the hunk.
        lines: vec![
            context("c1"),
            added("a1"),
            removed("r1"),
            context("c2"),
            removed("r2"),
        ],
    };

    let payload = render(rust(), std::slice::from_ref(&hunk)).expect("render");

    let expected: BTreeSet<u32> = [10, 11, 12].into_iter().collect();
    assert_eq!(
        payload.valid_lines, expected,
        "valid_lines should contain context and added numbers in file order, \
         no removed number - in particular not 13, which is what a trailing \
         removed line would contribute if removals were numbered"
    );
}

#[test]
fn two_hunks_with_a_gap_emit_a_single_omission_line() {
    let hunk_a = Hunk {
        file_path: PathBuf::from("wide.rs"),
        old_start: 1,
        old_count: 30,
        new_start: 1,
        new_count: 30,
        lines: (1..=30)
            .map(|n| context(&format!("line {n}")))
            .collect::<Vec<_>>(),
    };
    let hunk_b = Hunk {
        file_path: PathBuf::from("wide.rs"),
        old_start: 100,
        old_count: 1,
        new_start: 73,
        new_count: 1,
        lines: vec![context("line 73")],
    };

    let payload = render(rust(), &[hunk_a, hunk_b.clone()]).expect("render");

    let expected = "... 42 lines omitted ...\n";
    let occurrences = payload.text.matches(expected).count();
    assert_eq!(
        occurrences, 1,
        "expected exactly one gap line of `... 42 lines omitted ...`, got:\n{}",
        payload.text
    );
    assert!(
        payload.valid_lines.contains(&73),
        "valid_lines should include the second hunk's line"
    );
}

#[test]
fn adjacent_hunks_produce_no_omission_line() {
    let hunk_a = Hunk {
        file_path: PathBuf::from("touches.rs"),
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 5,
        lines: (1..=5)
            .map(|n| context(&format!("line {n}")))
            .collect::<Vec<_>>(),
    };
    let hunk_b = Hunk {
        file_path: PathBuf::from("touches.rs"),
        old_start: 10,
        old_count: 1,
        new_start: 6,
        new_count: 1,
        lines: vec![context("line 6")],
    };

    let payload = render(rust(), &[hunk_a, hunk_b]).expect("render");

    assert!(
        !payload.text.contains("lines omitted"),
        "adjacent hunks must not produce an omission line, got:\n{}",
        payload.text
    );
}

#[test]
fn whole_file_uses_the_whole_file_scope_sentence_diff_uses_marked_lines() {
    // Both sentences are asserted in the same test, by their exact text,
    // so a single hardcoded sentence cannot satisfy both.
    let whole = Hunk::whole_file(PathBuf::from("solo.rs"), "alpha\nbeta\ngamma");
    let diff_hunk = Hunk {
        file_path: PathBuf::from("delta.rs"),
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: 1,
        lines: vec![added("brand new")],
    };

    let whole_payload = render(rust(), std::slice::from_ref(&whole)).expect("whole render");
    let diff_payload = render(rust(), std::slice::from_ref(&diff_hunk)).expect("diff render");

    assert!(
        whole_payload.text.contains(
            "Review the entire file. Report each finding using the line number shown in the gutter."
        ),
        "whole-file payload must use the whole-file scope sentence, got:\n{}",
        whole_payload.text
    );
    assert!(
        diff_payload.text.contains(
            "Review the lines marked `+`. Lines with no marker are unchanged context. \
             Lines marked `-` were removed and have no line number; do not report findings \
             on them. Report each finding using the line number shown in the gutter."
        ),
        "diff payload must use the marked-lines scope sentence, got:\n{}",
        diff_payload.text
    );
}
