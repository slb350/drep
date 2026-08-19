//! `parse_unified_diff` and the `Hunk` constructors: criteria 1-13.

use std::path::PathBuf;

use crate::diff::hunks::{Hunk, HunkLine, parse_unified_diff};

#[test]
fn empty_and_whitespace_only_inputs_yield_no_hunks() {
    assert!(parse_unified_diff("").is_empty());
    assert!(parse_unified_diff("   \n\n  \n\t\n").is_empty());
}

#[test]
fn single_hunk_captures_all_four_header_numbers() {
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n\
                 index 1111111..2222222 100644\n\
                 --- a/src/lib.rs\n\
                 +++ b/src/lib.rs\n\
                 @@ -10,3 +10,4 @@ fn greet() {\n\
                     let name = \"world\";\n\
                 -    println!(\"hi {name}\");\n\
                 +    println!(\"hello, {name}!\");\n\
                     name.len()\n\
                 }";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1, "expected one hunk, got {hunks:?}");
    let h = &hunks[0];
    assert_eq!(h.old_start, 10);
    assert_eq!(h.old_count, 3);
    assert_eq!(h.new_start, 10);
    assert_eq!(h.new_count, 4);
}

#[test]
fn hunk_header_with_omitted_counts_defaults_to_one() {
    let diff = "diff --git a/foo.rs b/foo.rs\n\
                 --- a/foo.rs\n\
                 +++ b/foo.rs\n\
                 @@ -5 +5 @@\n\
                 -old\n\
                 +new\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(h.old_start, 5);
    assert_eq!(h.old_count, 1);
    assert_eq!(h.new_start, 5);
    assert_eq!(h.new_count, 1);
}

#[test]
fn new_file_header_has_zero_old_start_and_count() {
    let diff = "diff --git a/new.rs b/new.rs\n\
                 new file mode 100644\n\
                 index 0000000..1111111\n\
                 --- /dev/null\n\
                 +++ b/new.rs\n\
                 @@ -0,0 +1,3 @@\n\
                 +alpha\n\
                 +beta\n\
                 +gamma\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(h.old_start, 0);
    assert_eq!(h.old_count, 0);
    assert_eq!(h.new_start, 1);
    assert_eq!(h.new_count, 3);
}

#[test]
fn two_file_diff_attributes_each_hunk_to_its_file() {
    let diff = "diff --git a/alpha.rs b/alpha.rs\n\
                 --- a/alpha.rs\n\
                 +++ b/alpha.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -alpha-before\n\
                 +alpha-after\n\
                 diff --git a/beta.rs b/beta.rs\n\
                 --- a/beta.rs\n\
                 +++ b/beta.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -beta-before\n\
                 +beta-after\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].file_path, PathBuf::from("alpha.rs"));
    assert_eq!(hunks[1].file_path, PathBuf::from("beta.rs"));
}

#[test]
fn b_substring_in_a_repo_path_does_not_corrupt_file_path() {
    // The `diff --git` line here contains `b/` twice (the prefix and the
    // directory `b/` inside the path). The parser must read the path from
    // `+++ b/src/b/mod.rs` and produce `src/b/mod.rs`, not the substring
    // `b/mod.rs` from the `diff --git` line.
    let diff = "diff --git a/src/b/mod.rs b/src/b/mod.rs\n\
                 --- a/src/b/mod.rs\n\
                 +++ b/src/b/mod.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].file_path, PathBuf::from("src/b/mod.rs"));
}

#[test]
fn removed_line_whose_content_starts_with_dashes_is_preserved() {
    // The diff line is `--- legacy flag`. The parser must treat it as a
    // Removed line whose content is `-- legacy flag`, not as a header to
    // skip.
    let diff = "diff --git a/flags.rs b/flags.rs\n\
                 --- a/flags.rs\n\
                 +++ b/flags.rs\n\
                 @@ -1,3 +1,2 @@\n\
                 --- legacy flag\n\
                 kept\n\
                 -also kept\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    let lines = &hunks[0].lines;
    assert!(
        lines
            .iter()
            .any(|l| l == &HunkLine::Removed("-- legacy flag".to_owned())),
        "removed line with `--` prefix must be preserved, got {lines:?}"
    );
}

#[test]
fn added_line_whose_content_starts_with_plus_plus_is_preserved() {
    // The diff line is `+++ready`. The parser must treat it as an Added
    // line whose content is `++ready`, not as a header to skip.
    let diff = "diff --git a/heat.rs b/heat.rs\n\
                 --- a/heat.rs\n\
                 +++ b/heat.rs\n\
                 @@ -1,1 +1,2 @@\n\
                 +++ready\n\
                 still here\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    let lines = &hunks[0].lines;
    assert!(
        lines
            .iter()
            .any(|l| l == &HunkLine::Added("++ready".to_owned())),
        "added line with `++` prefix must be preserved, got {lines:?}"
    );
}

#[test]
fn no_newline_marker_is_not_a_hunk_line() {
    let diff = "diff --git a/tail.rs b/tail.rs\n\
                 --- a/tail.rs\n\
                 +++ b/tail.rs\n\
                 @@ -1,2 +1,2 @@\n\
                 -old line\n\
                 \\ No newline at end of file\n\
                 +new line\n\
                 \\ No newline at end of file\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(
        h.lines.len(),
        2,
        "the `\\` lines must not become HunkLines, got {h:?}"
    );
    assert!(h
        .lines
        .iter()
        .all(|l| !matches!(l, HunkLine::Removed(s) | HunkLine::Context(s) | HunkLine::Added(s) if s.contains("No newline"))));
}

#[test]
fn numbered_new_lines_skips_removed_lines_without_advancing() {
    let hunk = Hunk {
        file_path: PathBuf::from("demo.rs"),
        old_start: 99,
        old_count: 4,
        new_start: 100,
        new_count: 3,
        lines: vec![
            HunkLine::Context("context".to_owned()),
            HunkLine::Added("added".to_owned()),
            HunkLine::Removed("removed".to_owned()),
            HunkLine::Context("context".to_owned()),
        ],
    };

    let numbers: Vec<u32> = hunk.numbered_new_lines().map(|(n, _)| n).collect();
    assert_eq!(numbers, vec![100, 101, 102]);
}

#[test]
fn the_parser_is_free_of_scan_target_policy() {
    // Which files drep reviews is a product decision and lives in `mod.rs`
    // beside `filter_paths`, not in a mechanical diff parser. The
    // parser reports every file the diff mentions; `staged_hunks` is what
    // drops `Cargo.lock`, pinned by
    // `staged_hunks_returns_no_hunk_for_cargo_lock`. Keeping the policy out
    // of here is what lets a future caller with a different scope reuse the
    // parser without threading a predicate through it.
    let diff = "diff --git a/Cargo.lock b/Cargo.lock\n\
                 --- a/Cargo.lock\n\
                 +++ b/Cargo.lock\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n\
                 diff --git a/src/main.rs b/src/main.rs\n\
                 --- a/src/main.rs\n\
                 +++ b/src/main.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n";

    let hunks = parse_unified_diff(diff);
    let paths: Vec<&PathBuf> = hunks.iter().map(|h| &h.file_path).collect();
    assert_eq!(
        paths,
        vec![&PathBuf::from("Cargo.lock"), &PathBuf::from("src/main.rs"),],
        "the parser must report every file the diff mentions, applying no \
         scan-target policy of its own"
    );
}

#[test]
fn malformed_hunk_header_does_not_blend_into_a_previous_hunk() {
    let diff = "diff --git a/foo.rs b/foo.rs\n\
                 --- a/foo.rs\n\
                 +++ b/foo.rs\n\
                 @@ -1,2 +1,2 @@\n\
                 -first removed\n\
                 +first added\n\
                 @@garbage\n\
                 -stray body line that must not join hunk 1\n\
                 +more stray\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(
        hunks.len(),
        1,
        "only the first hunk is valid, got {hunks:?}"
    );
    let h = &hunks[0];
    assert_eq!(
        h.lines.len(),
        2,
        "the malformed header must not contribute a body, got {h:?}"
    );
    assert!(matches!(&h.lines[0], HunkLine::Removed(s) if s == "first removed"));
    assert!(matches!(&h.lines[1], HunkLine::Added(s) if s == "first added"));
}

#[test]
fn whole_file_yields_only_context_lines_starting_at_one() {
    let hunk = Hunk::whole_file(PathBuf::from("solo.rs"), "alpha\nbeta\ngamma");

    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_count, 3);
    assert_eq!(hunk.lines.len(), 3);
    assert!(hunk.lines.iter().all(|l| matches!(l, HunkLine::Context(_))));
    let numbers: Vec<u32> = hunk.numbered_new_lines().map(|(n, _)| n).collect();
    assert_eq!(numbers, vec![1, 2, 3]);
}
