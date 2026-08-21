//! Rule-based markdown checks. No LLM, no network, no configuration.
//!
//! Ten checks over the text of a markdown file, run by `drep lint-docs`. This
//! module deliberately imports nothing from `llm`, `config` or `analysis`
//! beyond the [`Finding`] vocabulary: `lint-docs` runs on every commit, and
//! the 1.x equivalent paid 190 ms of startup for a provider chain, a response
//! cache and a database it never touched.
//!
//! Structure:
//!
//! - `fence` answers "is this line inside a code fence", once per file, for
//!   every check that asks. See its module doc for why that is not a per-check
//!   concern.
//! - `lines` holds the checks that look at one line in isolation.
//! - `links` holds the two that need markdown's link grammar.
//! - `blocks` holds the three that span more than one line.
//!
//! The split is by what a check needs to see, not by file size, so a new check
//! has an obvious home.

mod blocks;
mod fence;
mod lines;
mod links;

use std::path::Path;

use crate::analysis::findings::{Finding, Severity};

/// Longest line drep will accept outside a code fence.
///
/// A fixed number, not a setting. `lint-docs` is report-only unless `--strict`
/// is passed, so a project that disagrees (this repository does - its
/// `.markdownlint.json` sets `MD013: false`) runs it report-only and ignores
/// the line rather than tuning a threshold drep would then have to reconcile
/// with the project's own linter.
pub const LONG_LINE_MAX: usize = 120;

/// Consecutive blank lines tolerated outside a code fence.
///
/// The check fires on the run that exceeds this, i.e. at three blanks.
pub const BLANK_RUN_MAX: usize = 2;

/// The ten checks.
///
/// The wire names are the `type=` strings the 1.x Python analyzer emitted, so
/// a user who scripted against `drep lint-docs` output does not have to relearn
/// them. [`Check::as_str`] is the only place a name is written.
///
/// [`Check::ALL`] is what the tests iterate, and it is a hand-maintained list:
/// Rust cannot enumerate an enum without a derive macro, so nothing makes a
/// new variant appear in it. What does force the author's hand is that the
/// three `match self` methods below are exhaustive - a new variant fails to
/// compile until all three are extended, and `ALL` sits immediately above
/// them. Treat adding to `ALL` as part of adding a variant; a check missing
/// from it silently drops out of every test in this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    BareUrl,
    EmptyHeading,
    LinkSyntaxInvalid,
    LongLine,
    MissingSpaceAfterHeading,
    MultipleBlankLines,
    TabCharacter,
    TrailingBlankLines,
    TrailingWhitespace,
    UnclosedCodeFence,
}

impl Check {
    /// Every check. The one place the vocabulary is listed.
    pub const ALL: [Check; 10] = [
        Check::BareUrl,
        Check::EmptyHeading,
        Check::LinkSyntaxInvalid,
        Check::LongLine,
        Check::MissingSpaceAfterHeading,
        Check::MultipleBlankLines,
        Check::TabCharacter,
        Check::TrailingBlankLines,
        Check::TrailingWhitespace,
        Check::UnclosedCodeFence,
    ];

    /// The wire name, as it appears in a finding's `kind`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Check::BareUrl => "bare_url",
            Check::EmptyHeading => "empty_heading",
            Check::LinkSyntaxInvalid => "link_syntax_invalid",
            Check::LongLine => "long_line",
            Check::MissingSpaceAfterHeading => "missing_space_after_heading",
            Check::MultipleBlankLines => "multiple_blank_lines",
            Check::TabCharacter => "tab_character",
            Check::TrailingBlankLines => "trailing_blank_lines",
            Check::TrailingWhitespace => "trailing_whitespace",
            Check::UnclosedCodeFence => "unclosed_code_fence",
        }
    }

    /// How badly this check's subject breaks the document.
    ///
    /// One rule decides all ten: **does it change how the document renders?**
    ///
    /// - [`Severity::Error`] - the rest of the file renders as something else.
    ///   Only an unclosed fence does that, and it does it to every line below
    ///   itself.
    /// - [`Severity::Warning`] - that line renders wrong. A heading that is not
    ///   a heading, a link that is not a link.
    /// - [`Severity::Info`] - renders identically; this is hygiene.
    ///
    /// The rule matters because `drep lint-docs --strict` and `--fail-on`
    /// downstream both gate on it, and "whitespace blocks a commit" is the
    /// calibration failure that makes a gate get switched off.
    pub const fn severity(self) -> Severity {
        match self {
            Check::UnclosedCodeFence => Severity::Error,
            Check::EmptyHeading | Check::MissingSpaceAfterHeading | Check::LinkSyntaxInvalid => {
                Severity::Warning
            }
            Check::BareUrl
            | Check::LongLine
            | Check::MultipleBlankLines
            | Check::TabCharacter
            | Check::TrailingBlankLines
            | Check::TrailingWhitespace => Severity::Info,
        }
    }

    /// What to do about it. Advice, never a literal replacement.
    ///
    /// 1.x carried a `replacement` field that was sometimes a rewritten line
    /// and sometimes a sentence of prose, because a draft-PR autofix consumed
    /// it. 2.0 has no autofix, so a field that is a literal half the time is a
    /// trap for whoever next tries to apply one.
    pub const fn suggestion(self) -> &'static str {
        match self {
            Check::BareUrl => "wrap it as [text](url)",
            Check::EmptyHeading => "give the heading text, or delete the line",
            Check::LinkSyntaxInvalid => "balance the brackets: [text](url)",
            Check::LongLine => "wrap or rephrase",
            Check::MissingSpaceAfterHeading => "put a space after the `#`s",
            Check::MultipleBlankLines => "reduce to one blank line",
            Check::TabCharacter => "replace tabs with spaces",
            Check::TrailingBlankLines => "remove the blank line(s) at end of file",
            Check::TrailingWhitespace => "remove the trailing whitespace",
            Check::UnclosedCodeFence => "close it with ```",
        }
    }
}

/// One line, and where it sits.
///
/// Deliberately does **not** carry the line's characters. Every column drep
/// reports is a character offset rather than a byte offset - a heading under a
/// line containing an em dash must not have its column shifted by two - so the
/// checks do need a `[char]`, but only for the line being examined. Holding one
/// `Vec<char>` per line meant an allocation per line of every file and 43% of
/// the analysis pass; [`analyze`] now fills a single reused buffer instead.
/// [`blocks`] needs no characters at all.
pub(crate) struct Line<'a> {
    /// 1-based line number, as reported.
    pub number: u32,
    /// The line as written, without its terminator.
    pub text: &'a str,
    /// True iff a fence delimiter or a line between two of them.
    pub in_fence: bool,
}

/// Build a [`Finding`] for `check` at a position.
///
/// Central so that the kind/severity/suggestion triple is never assembled by
/// hand at a check site, where one of the three can quietly disagree with
/// [`Check`].
pub(crate) fn finding(
    check: Check,
    file_path: &str,
    line: u32,
    column: u32,
    message: String,
) -> Finding {
    Finding::deterministic(
        check.as_str().to_owned(),
        check.severity(),
        file_path.to_owned(),
        line,
        Some(column),
        message,
        Some(check.suggestion().to_owned()),
    )
}

/// Run every check over `content`, reporting against `path`.
///
/// Findings come back sorted by position, then by check name, so the output of
/// two runs over the same file is byte-identical and a reader follows the file
/// top to bottom. The checks themselves run in whatever order is convenient -
/// grouping the output by check, as the 1.x implementation's append order did,
/// makes a reader jump around the file.
pub fn analyze(path: &Path, content: &str) -> Vec<Finding> {
    let file_path = path.to_string_lossy().into_owned();
    let raw: Vec<&str> = content.lines().collect();
    let fences = fence::Fences::scan(&raw);

    let lines: Vec<Line<'_>> = raw
        .iter()
        .zip(fences.mask())
        .enumerate()
        .map(|(index, (text, in_fence))| Line {
            number: index as u32 + 1,
            text,
            in_fence: *in_fence,
        })
        .collect();

    // Two buffers for the whole file rather than one allocation per line:
    // `chars` holds the line under examination, `scratch` the blanked copy the
    // link checks work on. Both are cleared and refilled, so peak memory is the
    // longest line rather than the file.
    let mut findings = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    let mut scratch: Vec<char> = Vec::new();
    for line in &lines {
        chars.clear();
        chars.extend(line.text.chars());
        lines::check(line, &chars, &file_path, &mut findings);
        links::check(line, &chars, &mut scratch, &file_path, &mut findings);
    }
    blocks::check(&lines, &fences, &file_path, &mut findings);

    findings.sort_by(|a, b| (a.line, a.column, &a.kind).cmp(&(b.line, b.column, &b.kind)));
    findings
}

#[cfg(test)]
mod tests;
