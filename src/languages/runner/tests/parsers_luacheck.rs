//! luacheck output through the shared `position` parser.
//!
//! Every input below is captured verbatim from Luacheck 1.2.0 invoked with
//! exactly the argv the Lua registry entry builds:
//! `luacheck --formatter plain --codes --no-color`. The parser is shared
//! with `go vet`, so these tests construct their own Lua-shaped spec and
//! leave the shared fixtures alone.

use super::support::*;
use crate::analysis::findings::Severity;
use crate::languages::runner::*;

/// The full warnings run: exit status 1, five findings in file order, with
/// the parenthesised luacheck code landing inside the message group.
#[test]
fn warning_run_parses_into_five_findings_in_order() {
    let spec = luacheck_like_spec();
    let output = concat!(
        "sample.lua:1:28: (W212) unused argument 'unused_arg'\n",
        "sample.lua:3:3: (W113) accessing undefined variable 'undefined_global_call'\n",
        "sample.lua:7:7: (W231) variable 'x' is never accessed\n",
        "sample.lua:11:9: (W211) unused variable 'i'\n",
        "sample.lua:11:9: (W413) variable 'i' was previously defined as a loop variable on line 10\n",
    );
    let findings = parse_output(&spec, output, "root").expect("captured luacheck output parses");
    assert_eq!(findings.len(), 5);
    assert_eq!(findings[0].kind, "luacheck");
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].file_path, "sample.lua");
    assert_eq!(findings[0].line, 1);
    assert_eq!(findings[0].column, Some(28));
    assert_eq!(findings[0].message, "(W212) unused argument 'unused_arg'");
    assert!(findings[0].suggestion.is_none());
    assert!(!findings[0].asserts_compile_failure);
    assert!(findings[0].fingerprint.is_none());
    assert_eq!(findings[4].file_path, "sample.lua");
    assert_eq!(findings[4].line, 11);
    assert_eq!(findings[4].column, Some(9));
    assert_eq!(
        findings[4].message,
        "(W413) variable 'i' was previously defined as a loop variable on line 10"
    );
}

/// The captured run reports `(W211)` and `(W413)` at the same
/// `sample.lua:11:9`; both must survive as distinct findings, because a
/// parser that deduplicated by position would drop one of them.
#[test]
fn two_findings_on_one_line_and_column_both_survive() {
    let spec = luacheck_like_spec();
    let output = concat!(
        "sample.lua:11:9: (W211) unused variable 'i'\n",
        "sample.lua:11:9: (W413) variable 'i' was previously defined as a loop variable on line 10\n",
    );
    let findings = parse_output(&spec, output, "root").expect("captured luacheck output parses");
    assert_eq!(findings.len(), 2);
    for finding in &findings {
        assert_eq!(finding.file_path, "sample.lua");
        assert_eq!(finding.line, 11);
        assert_eq!(finding.column, Some(9));
    }
    assert_eq!(findings[0].message, "(W211) unused variable 'i'");
    assert_eq!(
        findings[1].message,
        "(W413) variable 'i' was previously defined as a loop variable on line 10"
    );
}

/// The syntax-error run: exit status 2, one finding. The message contains
/// an apostrophe and parentheses, so it is asserted as the exact captured
/// text - a naive quote- or paren-stripping change fails here.
#[test]
fn syntax_error_line_parses() {
    let spec = luacheck_like_spec();
    let findings = parse_output(
        &spec,
        "bad.lua:2:10: (E011) expected '(' near 'end'\n",
        "root",
    )
    .expect("captured luacheck output parses");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "bad.lua");
    assert_eq!(findings[0].line, 2);
    assert_eq!(findings[0].column, Some(10));
    assert_eq!(findings[0].message, "(E011) expected '(' near 'end'");
}

/// The unreadable-file run: exit status 3, a line with no `:line:col:`
/// suffix. The parser must skip it, because `Position` is in the
/// `skips_unmatched_input` set - a non-zero exit with zero findings on a
/// non-empty stream is `Unavailable`, so a luacheck that never read the
/// file is reported unanalyzable rather than silently clean. A parser that
/// matched this line would manufacture a finding on a file it never read.
#[test]
fn io_error_line_yields_no_finding() {
    let spec = luacheck_like_spec();
    let findings = parse_output(
        &spec,
        "nope.lua: I/O error (couldn't read: No such file or directory)\n",
        "root",
    )
    .expect("captured luacheck output parses");
    assert!(findings.is_empty());
}

/// The `plain` formatter emits no summary trailer, so a clean run is a
/// completely empty stream - which the runner's empty-stream guard treats
/// as a clean pass.
#[test]
fn clean_run_yields_no_findings() {
    let spec = luacheck_like_spec();
    let findings = parse_output(&spec, "", "root").expect("empty output parses");
    assert!(findings.is_empty());
}

/// luacheck echoes each path exactly as it was passed on argv, so a
/// leading slash and every path segment must survive verbatim, with no
/// `./` stripping applied.
#[test]
fn absolute_path_is_preserved_verbatim() {
    let spec = luacheck_like_spec();
    let findings = parse_output(
        &spec,
        "/home/u/proj/init.lua:3:1: (W211) unused variable 'x'\n",
        "root",
    )
    .expect("captured luacheck output parses");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file_path, "/home/u/proj/init.lua");
    assert_eq!(findings[0].line, 3);
    assert_eq!(findings[0].column, Some(1));
    assert_eq!(findings[0].message, "(W211) unused variable 'x'");
}
