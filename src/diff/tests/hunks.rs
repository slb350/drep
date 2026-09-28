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

#[test]
fn a_deleted_files_header_attributes_its_hunks_to_nothing() {
    // `+++ /dev/null` names no new file, so the deletion's body is dropped
    // rather than filed under the previous file or a path called `/dev/null`.
    let diff = "diff --git a/kept.rs b/kept.rs\n\
                 --- a/kept.rs\n\
                 +++ b/kept.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n\
                 diff --git a/gone.rs b/gone.rs\n\
                 deleted file mode 100644\n\
                 --- a/gone.rs\n\
                 +++ /dev/null\n\
                 @@ -1,1 +0,0 @@\n\
                 -removed\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1, "only kept.rs has a new side, got {hunks:?}");
    assert_eq!(hunks[0].file_path, PathBuf::from("kept.rs"));
    assert_eq!(hunks[0].lines.len(), 2);
}

/// Git C-quotes a path holding a byte `core.quotePath` escapes, including its
/// `b/` prefix, so the header does not begin `+++ b/`. Read literally, the
/// file matched no language and its change was never reviewed.
#[test]
fn a_quoted_header_names_the_decoded_path() {
    let diff = "diff --git \"a/src/caf\\303\\251.rs\" \"b/src/caf\\303\\251.rs\"\n\
                 --- \"a/src/caf\\303\\251.rs\"\n\
                 +++ \"b/src/caf\\303\\251.rs\"\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1, "got {hunks:?}");
    assert_eq!(hunks[0].file_path, PathBuf::from("src/caf\u{e9}.rs"));
}

/// Git ends a `---`/`+++` header with a tab whenever the name holds a space,
/// quoted or not. Kept, the tab became part of the extension (`.rs\t`).
#[test]
fn a_header_naming_a_path_with_a_space_drops_the_tab_git_appends() {
    let diff = "diff --git a/src/a b.rs b/src/a b.rs\n\
                 --- a/src/a b.rs\t\n\
                 +++ b/src/a b.rs\t\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n\
                 diff --git \"a/c d\\303\\251.rs\" \"b/c d\\303\\251.rs\"\n\
                 --- \"a/c d\\303\\251.rs\"\t\n\
                 +++ \"b/c d\\303\\251.rs\"\t\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n";

    let paths: Vec<PathBuf> = parse_unified_diff(diff)
        .into_iter()
        .map(|hunk| hunk.file_path)
        .collect();
    assert_eq!(
        paths,
        vec![PathBuf::from("src/a b.rs"), PathBuf::from("c d\u{e9}.rs")]
    );
}

/// A file header appears only before a file's first `@@`. Inside a body the
/// first byte decides the line kind, so added lines whose text is `++ b/...`
/// or `++ /dev/null` are code, not a switch to another file or a deletion.
#[test]
fn an_added_line_that_reads_like_a_file_header_stays_in_its_hunk() {
    let diff = "diff --git a/patch.rs b/patch.rs\n\
                 --- a/patch.rs\n\
                 +++ b/patch.rs\n\
                 @@ -1,1 +1,4 @@\n\
                 +++ b/other.rs\n\
                 +++ /dev/null\n\
                 +tail\n\
                 \x20kept\n";

    let hunks = parse_unified_diff(diff);
    assert_eq!(hunks.len(), 1, "got {hunks:?}");
    assert_eq!(hunks[0].file_path, PathBuf::from("patch.rs"));
    assert_eq!(
        hunks[0].lines,
        vec![
            HunkLine::Added("++ b/other.rs".to_owned()),
            HunkLine::Added("++ /dev/null".to_owned()),
            HunkLine::Added("tail".to_owned()),
            HunkLine::Context("kept".to_owned()),
        ]
    );
}

/// With `diff.noprefix` or `diff.mnemonicPrefix` a header lacks the `b/` the
/// parser requires. drep pins the prefix on every `git diff` it runs, so an
/// unprefixed name can only come from some other producer: it is not guessed
/// at, because `src/lib.rs` would otherwise be read as `lib.rs` under `src/`.
#[test]
fn a_header_without_the_destination_prefix_names_no_file() {
    let diff = "diff --git src/lib.rs src/lib.rs\n\
                 --- src/lib.rs\n\
                 +++ src/lib.rs\n\
                 @@ -1,1 +1,1 @@\n\
                 -old\n\
                 +new\n";

    assert!(parse_unified_diff(diff).is_empty());
}

/// Join diff lines exactly, so a context line keeps its leading space and an
/// empty line stays empty; a `\`-continued string literal strips both.
fn diff_of(lines: &[&str]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[test]
fn an_added_line_spelling_a_file_header_does_not_reattribute_later_hunks() {
    let diff = diff_of(&[
        "diff --git a/lib.rs b/lib.rs",
        "index 1111111..2222222 100644",
        "--- a/lib.rs",
        "+++ b/lib.rs",
        "@@ -1,2 +1,3 @@",
        " const DOC: &str = r\"",
        "+++ b/decoy.rs",
        " \";",
        "@@ -40,2 +41,3 @@ fn tail() {",
        " fn tail() {",
        "+    hidden();",
        " }",
    ]);

    let hunks = parse_unified_diff(&diff);

    let paths: Vec<&PathBuf> = hunks.iter().map(|h| &h.file_path).collect();
    assert_eq!(
        paths,
        vec![&PathBuf::from("lib.rs"), &PathBuf::from("lib.rs")],
        "both hunks belong to lib.rs"
    );
    assert_eq!(
        hunks[0].lines[1],
        HunkLine::Added("++ b/decoy.rs".to_owned())
    );
    assert_eq!(
        hunks[1].lines[1],
        HunkLine::Added("    hidden();".to_owned())
    );
}

#[test]
fn a_header_after_a_complete_hunk_body_starts_the_next_file() {
    // Concatenated unified diffs with no `diff --git` line between files: the
    // declared counts, not a later header, decide where the body ends.
    let diff = diff_of(&[
        "--- a/one.rs",
        "+++ b/one.rs",
        "@@ -1,2 +1,2 @@",
        "-a",
        "+b",
        " c",
        "--- a/two.rs",
        "+++ b/two.rs",
        "@@ -1 +1 @@",
        "-d",
        "+e",
    ]);

    let hunks = parse_unified_diff(&diff);

    assert_eq!(hunks.len(), 2, "got {hunks:?}");
    assert_eq!(hunks[0].file_path, PathBuf::from("one.rs"));
    assert_eq!(
        hunks[0].lines,
        vec![
            HunkLine::Removed("a".to_owned()),
            HunkLine::Added("b".to_owned()),
            HunkLine::Context("c".to_owned()),
        ]
    );
    assert_eq!(hunks[1].file_path, PathBuf::from("two.rs"));
}

#[test]
fn a_body_line_beyond_the_declared_counts_ends_the_hunk_without_panicking() {
    let diff = diff_of(&[
        "--- a/x.rs",
        "+++ b/x.rs",
        "@@ -1 +1,2 @@",
        "-a",
        "-surplus",
        "+b",
    ]);

    let hunks = parse_unified_diff(&diff);

    assert_eq!(hunks.len(), 1, "got {hunks:?}");
    assert_eq!(hunks[0].lines, vec![HunkLine::Removed("a".to_owned())]);
}

#[test]
fn an_empty_body_line_is_an_empty_context_line() {
    // `diff.suppressBlankEmpty` drops the space git otherwise prints before an
    // empty context line. It still occupies a line on both sides.
    let diff = diff_of(&[
        "--- a/gap.rs",
        "+++ b/gap.rs",
        "@@ -1,3 +1,3 @@",
        " fn a() {}",
        "",
        "-old();",
        "+new();",
    ]);

    let hunks = parse_unified_diff(&diff);

    assert_eq!(hunks.len(), 1, "got {hunks:?}");
    assert_eq!(hunks[0].lines[1], HunkLine::Context(String::new()));
    let numbered: Vec<(u32, &str)> = hunks[0].numbered_new_lines().collect();
    assert_eq!(numbered, vec![(1, "fn a() {}"), (2, ""), (3, "new();")]);
}

#[test]
fn every_body_line_kind_counts_against_its_own_side() {
    // Context consumes both sides, a removal only the old side and an
    // addition only the new side, while the no-newline marker consumes
    // neither. The trailing `-` and `+` would be absorbed into this hunk if
    // any kind were counted against the wrong side.
    let diff = diff_of(&[
        "--- a/sides.rs",
        "+++ b/sides.rs",
        "@@ -1,3 +1,3 @@",
        " keep",
        "-gone",
        "\\ No newline at end of file",
        "+came",
        " last",
        "-beyond the old side",
        "+beyond the new side",
    ]);

    let hunks = parse_unified_diff(&diff);

    assert_eq!(hunks.len(), 1, "got {hunks:?}");
    assert_eq!(
        hunks[0].lines,
        vec![
            HunkLine::Context("keep".to_owned()),
            HunkLine::Removed("gone".to_owned()),
            HunkLine::Added("came".to_owned()),
            HunkLine::Context("last".to_owned()),
        ]
    );
}

#[test]
fn a_hunk_runs_until_both_sides_are_used_up() {
    // One side of a hunk often runs out first: the removals at the end of a
    // shortened file come after the last new line, and the additions at the
    // end of a lengthened one after the last old line. Ending the body with
    // either side would drop those lines from what the model reads.
    let diff = diff_of(&[
        "--- a/tail.rs",
        "+++ b/tail.rs",
        "@@ -1,3 +1 @@",
        " keep",
        "-dropped one",
        "-dropped two",
        "@@ -10 +8,3 @@",
        " keep",
        "+added one",
        "+added two",
    ]);

    let hunks = parse_unified_diff(&diff);

    let lines: Vec<&[HunkLine]> = hunks.iter().map(|h| h.lines.as_slice()).collect();
    assert_eq!(
        lines,
        vec![
            &[
                HunkLine::Context("keep".to_owned()),
                HunkLine::Removed("dropped one".to_owned()),
                HunkLine::Removed("dropped two".to_owned()),
            ][..],
            &[
                HunkLine::Context("keep".to_owned()),
                HunkLine::Added("added one".to_owned()),
                HunkLine::Added("added two".to_owned()),
            ][..],
        ]
    );
}
