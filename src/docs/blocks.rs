//! The three checks that span more than one line.

use crate::analysis::findings::Finding;
use crate::docs::fence::Fences;
use crate::docs::{BLANK_RUN_MAX, Check, Line, finding};

/// Run the multi-line checks over the whole file.
pub fn check(lines: &[Line<'_>], fences: &Fences, file_path: &str, out: &mut Vec<Finding>) {
    multiple_blank_lines(lines, file_path, out);
    trailing_blank_lines(lines, file_path, out);
    unclosed_code_fence(lines, fences, file_path, out);
}

/// A line with nothing but whitespace on it.
fn is_blank(line: &Line<'_>) -> bool {
    line.text.trim().is_empty()
}

/// More than [`BLANK_RUN_MAX`] blank lines in a row.
///
/// Reported once per run, at the run's **first** blank line, which is where a
/// fix starts deleting. Reporting at the line that tripped the threshold would
/// point two lines past the problem, and reporting on every line of a ten-blank
/// run would bury the rest of the file's findings.
///
/// The run resets inside a code fence rather than merely being skipped: blank
/// lines above a fence and blank lines below it are not one run, and treating
/// them as one would report a defect that spans code the check cannot see.
fn multiple_blank_lines(lines: &[Line<'_>], file_path: &str, out: &mut Vec<Finding>) {
    let mut run: usize = 0;
    for line in lines {
        if line.in_fence || !is_blank(line) {
            run = 0;
            continue;
        }
        run += 1;
        if run == BLANK_RUN_MAX + 1 {
            out.push(finding(
                Check::MultipleBlankLines,
                file_path,
                line.number - BLANK_RUN_MAX as u32,
                1,
                format!("more than {BLANK_RUN_MAX} consecutive blank lines"),
            ));
        }
    }
}

/// A file whose last line is blank.
///
/// Note what this is *not*: a file ending in a single `\n` has no blank last
/// line, because the terminator belongs to the last line of text. The check
/// fires on `...text\n\n`, which is a real trailing blank, and stays silent on
/// the well-formed `...text\n` that every tool in the toolchain produces.
///
/// Fires inside a fence too - an unclosed fence at end of file does not make
/// trailing blank lines acceptable, and the unclosed fence is reported
/// separately anyway.
fn trailing_blank_lines(lines: &[Line<'_>], file_path: &str, out: &mut Vec<Finding>) {
    let Some(last) = lines.last() else {
        return;
    };
    if !is_blank(last) {
        return;
    }
    let count = lines.iter().rev().take_while(|l| is_blank(l)).count();
    out.push(finding(
        Check::TrailingBlankLines,
        file_path,
        last.number,
        1,
        format!("file ends with {count} blank line(s)"),
    ));
}

/// An odd number of fence delimiters leaves the last one open.
///
/// Reported at that last delimiter, which is where the unterminated block
/// starts - not at end of file, where the symptom is. Severity is
/// [`crate::analysis::findings::Severity::Error`] alone among the ten, because
/// every line below this one renders as code.
fn unclosed_code_fence(
    lines: &[Line<'_>],
    fences: &Fences,
    file_path: &str,
    out: &mut Vec<Finding>,
) {
    let delimiters = fences.delimiters();
    if delimiters.len() % 2 == 0 {
        return;
    }
    let opener = *delimiters
        .last()
        .expect("an odd count is at least one delimiter");
    let text = lines
        .get(opener as usize - 1)
        .map_or("", |line| line.text.trim());
    out.push(finding(
        Check::UnclosedCodeFence,
        file_path,
        opener,
        1,
        format!("code fence `{text}` is never closed"),
    ));
}
