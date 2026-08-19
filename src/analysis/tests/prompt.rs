//! `prompt::build_analysis_prompt` — criteria 1-4.

use crate::analysis::prompt::build_analysis_prompt;

/// The prompt's first line must name the language.
///
/// Checked on line 1 rather than with a bare `contains`: the conventions
/// heading is `**<display_name>-specific concerns:**`, so `contains("Go")` is
/// satisfied by the heading alone and cannot tell a prompt rendered for this
/// language from one that never read `display_name`. Line 1 carries no
/// heading, and asserting "line 1 mentions the language" rather than the whole
/// opening sentence leaves the wording free to change.
fn assert_opening_names(prompt: &str, display_name: &str) {
    let first = prompt.lines().next().unwrap_or_default();
    assert!(
        first.contains(display_name),
        "the prompt must open by naming the language `{display_name}`, got first line: {first}"
    );
}
use crate::languages::definitions::{GO, PYTHON};
use crate::languages::spec::LanguageSupport;

/// Criterion 1a: the Python prompt carries Python's display name and every
/// one of its conventions entries. A prompt template hardcoded to one
/// language could pass Go's test by accident; asserting names per-language
/// rules that out.
#[test]
fn python_prompt_carries_display_name_and_every_convention() {
    let prompt = build_analysis_prompt(&PYTHON);

    assert_opening_names(&prompt, PYTHON.display_name);
    for convention in PYTHON.conventions {
        assert!(
            prompt.contains(convention),
            "Python prompt must list every convention entry, missing `{convention}` in:\n{prompt}"
        );
    }
}

/// Criterion 1b: the Go prompt carries Go's display name and every one of
/// its conventions. Separate from the Python test so a one-language
/// hardcoded template cannot satisfy both.
#[test]
fn go_prompt_carries_display_name_and_every_convention() {
    let prompt = build_analysis_prompt(&GO);

    assert_opening_names(&prompt, GO.display_name);
    for convention in GO.conventions {
        assert!(
            prompt.contains(convention),
            "Go prompt must list every convention entry, missing `{convention}` in:\n{prompt}"
        );
    }
}

/// Criterion 2: a language with no conventions must produce no
/// `Go-specific concerns:` heading, no `-specific concerns:` heading at all,
/// and no doubled blank line where the heading would have been.
#[test]
fn language_with_no_conventions_omits_the_concerns_block() {
    let lang = LanguageSupport {
        name: "anon",
        display_name: "Anon",
        extensions: &[".anon"],
        tools: &[],
        conventions: &[],
        vendored_dirs: &[],
    };

    let prompt = build_analysis_prompt(&lang);

    assert!(
        !prompt.contains("-specific concerns:"),
        "an empty conventions list must drop the heading entirely, got:\n{prompt}"
    );
    // Structural, not a literal: assert no two consecutive blank lines
    // anywhere. Pinning a substring that spans the *categories* paragraph
    // would couple this test to wording it has no stake in, and a blanket
    // `!contains("\n\n\n")` is the same check expressed less precisely.
    let doubled = prompt
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].trim().is_empty() && pair[1].trim().is_empty());
    assert!(
        !doubled,
        "an empty conventions block must not leave a doubled blank line, got:\n{prompt}"
    );

    // The populated case must still render the heading, or "no heading" would
    // pass for a build that never emits one.
    let populated = build_analysis_prompt(&PYTHON);
    assert!(
        populated.contains("**Python-specific concerns:**"),
        "a language with conventions must get the heading, got:\n{populated}"
    );
}

/// Criterion 3: the prompt contains the "do not report anything a formatter
/// or linter would catch" instruction. This is the line that prevents the
/// model from flooding every file with lint noise, and the deterministic
/// layer cannot survive without it.
#[test]
fn prompt_includes_the_linter_separation_instruction() {
    let prompt = build_analysis_prompt(&PYTHON);

    assert!(
        prompt.contains(
            "Do not report subjective style issues, and do not report anything a\n\
             formatter or linter would catch"
        ),
        "the prompt must instruct the model to defer to the linter layer, got:\n{prompt}"
    );
}

/// Criterion 4: the prompt tells the model to use the gutter line number
/// for `line`, not an offset into the excerpt. This is the whole reason
/// `payload::Payload::valid_lines` exists: the model is told the format
/// once, and the parser enforces it.
#[test]
fn prompt_tells_the_model_to_use_the_gutter_line_number() {
    let prompt = build_analysis_prompt(&PYTHON);

    assert!(
        prompt.contains("gutter"),
        "the prompt must refer to the gutter line number, got:\n{prompt}"
    );
    assert!(
        prompt.contains("line-numbered excerpt"),
        "the prompt must state the input is a line-numbered excerpt, got:\n{prompt}"
    );
}
