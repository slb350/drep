//! Conservative recognition of foreign `core.hooksPath` forwarders.

/// Whether a foreign shell hook directly invokes the repository-local hook.
///
/// Drep must not rewrite a foreign hook, but merely printing or testing a path
/// is not forwarding. Recognising the executable word after an optional `exec`
/// accepts the ordinary direct forms while refusing to infer execution from an
/// arbitrary mention later in a command.
pub(super) fn appears_to_forward(body: &str, name: &str) -> bool {
    let marker = format!("hooks/{name}");
    body.lines().any(|line| {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let command = line
            .strip_prefix("exec")
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .map_or(line, str::trim_start);
        shell_word(command).is_some_and(|word| {
            word.trim_matches(|character| matches!(character, '\'' | '"'))
                .ends_with(&marker)
        })
    })
}

/// The first shell word, keeping whitespace inside quotes and `$()` together.
fn shell_word(command: &str) -> Option<&str> {
    let mut quote = None;
    let mut substitution_depth = 0usize;
    let mut escaped = false;
    let mut previous = None;

    for (index, character) in command.char_indices() {
        if escaped {
            escaped = false;
            previous = None;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            previous = Some(character);
            continue;
        }
        match quote {
            Some(active) if character == active => quote = None,
            Some(_) => {}
            None if matches!(character, '\'' | '"') => quote = Some(character),
            None if character == '(' && previous == Some('$') => substitution_depth += 1,
            None if character == ')' && substitution_depth > 0 => substitution_depth -= 1,
            None if character.is_whitespace() && substitution_depth == 0 => {
                return (index > 0).then_some(&command[..index]);
            }
            None => {}
        }
        previous = Some(character);
    }
    (!command.is_empty() && quote.is_none() && substitution_depth == 0 && !escaped)
        .then_some(command)
}

#[cfg(test)]
mod tests {
    use super::{appears_to_forward, shell_word};

    #[test]
    fn a_comment_cannot_be_the_forwarding_executable() {
        assert!(!appears_to_forward(
            "#!/bin/sh\n#hooks/pre-push\nexit 0\n",
            "pre-push"
        ));
    }

    #[test]
    fn an_escaped_space_can_be_part_of_the_forwarding_executable() {
        let command = r#"/tmp/shared\ hooks/hooks/pre-push "$@""#;
        assert_eq!(
            shell_word(command),
            Some(r#"/tmp/shared\ hooks/hooks/pre-push"#)
        );
        assert!(appears_to_forward(&format!("exec {command}"), "pre-push"));
    }

    #[test]
    fn a_quoted_space_can_be_part_of_the_forwarding_executable() {
        let command = r#""/tmp/shared hooks/hooks/pre-push" "$@""#;
        assert_eq!(
            shell_word(command),
            Some(r#""/tmp/shared hooks/hooks/pre-push""#)
        );
        assert!(appears_to_forward(&format!("exec {command}"), "pre-push"));
    }

    #[test]
    fn a_single_quoted_space_can_be_part_of_the_forwarding_executable() {
        let command = "'/tmp/shared hooks/hooks/pre-push' \"$@\"";
        assert_eq!(
            shell_word(command),
            Some("'/tmp/shared hooks/hooks/pre-push'")
        );
        assert!(appears_to_forward(&format!("exec {command}"), "pre-push"));
    }

    #[test]
    fn an_unquoted_command_substitution_stays_in_the_executable_word() {
        let command = r#"$(git rev-parse --git-common-dir)/hooks/pre-push "$@""#;
        assert_eq!(
            shell_word(command),
            Some("$(git rev-parse --git-common-dir)/hooks/pre-push")
        );
        assert!(appears_to_forward(&format!("exec {command}"), "pre-push"));
    }

    #[test]
    fn nested_command_substitutions_balance_before_word_termination() {
        assert_eq!(
            shell_word("$(outer $(inner value))/hooks/pre-push rest"),
            Some("$(outer $(inner value))/hooks/pre-push")
        );
    }

    #[test]
    fn a_literal_parenthesis_does_not_open_a_command_substitution() {
        assert_eq!(shell_word("helper(arg hooks/pre-push"), Some("helper(arg"));
    }

    #[test]
    fn an_escaped_dollar_does_not_open_a_command_substitution() {
        assert_eq!(
            shell_word(r"helper\$(arg hooks/pre-push"),
            Some(r"helper\$(arg")
        );
    }

    #[test]
    fn unbalanced_shell_syntax_has_no_executable_word() {
        for command in [
            r#""hooks/pre-push"#,
            "'hooks/pre-push",
            "$(printf hooks/pre-push",
            r"hooks/pre-push\",
        ] {
            assert_eq!(shell_word(command), None, "command: {command}");
            assert!(
                !appears_to_forward(command, "pre-push"),
                "command: {command}"
            );
        }
    }

    #[test]
    fn an_empty_command_has_no_executable_word() {
        assert_eq!(shell_word(""), None);
    }
}
