//! Command-line surface.
//!
//! Four commands, two triggers (pre-commit and pre-push). Anything that needs
//! a platform API, a webhook or a database was dropped in 2.0 - see
//! `docs/rust-migration.md`.
//!
//! Each command owns its arguments in its own module, so a command's contract
//! and its behaviour stay together as the later phases fill them in.

pub mod check;
pub mod lint_docs;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use crate::Exit;
use check::CheckArgs;
use lint_docs::LintDocsArgs;

#[derive(Debug, Parser)]
#[command(
    name = "drep",
    version,
    about = "Run the linters your repo configures, and have an LLM review what changed",
    long_about = None,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Analyze local files. Intended for pre-commit and pre-push hooks.
    Check(CheckArgs),
    /// Lint markdown. Rule-based only - no LLM, no network.
    LintDocs(LintDocsArgs),
    /// Report which languages and tools drep can see in this repository.
    Doctor,
    /// Write the git hooks and LLM endpoint configuration.
    Init,
}

/// How findings are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable, for a terminal.
    Text,
    /// Machine-readable. Carries `unanalyzed` alongside `findings`, so a
    /// consumer can tell a clean run from one that never happened.
    Json,
}

/// Dispatch a parsed command.
pub async fn run(cli: Cli) -> Result<Exit> {
    match cli.command {
        Command::Check(args) => check::run(&args, std::path::Path::new(".")).await,
        Command::LintDocs(_) => unimplemented("lint-docs", "phase 6"),
        Command::Doctor => unimplemented("doctor", "phase 5"),
        Command::Init => unimplemented("init", "phase 5"),
    }
}

/// Fail loudly rather than exiting 0, so a hook wired up against a
/// half-finished build blocks instead of silently passing every commit.
fn unimplemented(command: &str, phase: &str) -> Result<Exit> {
    anyhow::bail!("`drep {command}` is not implemented yet (lands in {phase})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::findings::Severity;
    use clap::CommandFactory;

    /// Parse a `check` invocation and hand back its arguments.
    fn check_args<const N: usize>(argv: [&str; N]) -> CheckArgs {
        let cli = Cli::try_parse_from(argv).expect("should parse");
        match cli.command {
            Command::Check(args) => args,
            other => panic!("expected check, got {other:?}"),
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn input_modes_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["drep", "check", "--staged", "a.rs"]).is_err());
        assert!(Cli::try_parse_from(["drep", "check", "--staged", "--diff", "main"]).is_err());
        assert!(Cli::try_parse_from(["drep", "check", "--diff", "main", "a.rs"]).is_err());
    }

    #[test]
    fn each_input_mode_parses_alone() {
        assert_eq!(check_args(["drep", "check", "a.rs", "src/"]).paths.len(), 2);
        assert!(check_args(["drep", "check", "--staged"]).staged);
        assert_eq!(
            check_args(["drep", "check", "--diff", "origin/main"])
                .diff
                .as_deref(),
            Some("origin/main")
        );
        assert!(check_args(["drep", "check"]).paths.is_empty());
    }

    #[test]
    fn format_defaults_to_text_and_fail_on_defaults_to_off() {
        let args = check_args(["drep", "check"]);
        assert_eq!(args.format, OutputFormat::Text);
        assert_eq!(args.fail_on, None);
    }

    #[test]
    fn fail_on_accepts_the_whole_severity_vocabulary() {
        // Driven off `Severity::ALL` so a new severity is covered here the
        // moment it is added, rather than passing on a stale subset.
        for expected in Severity::ALL {
            let args = check_args(["drep", "check", "--fail-on", expected.as_str()]);
            assert_eq!(args.fail_on, Some(expected));
        }
        assert!(Cli::try_parse_from(["drep", "check", "--fail-on", "critical"]).is_err());
    }

    #[tokio::test]
    async fn unimplemented_commands_error_rather_than_exiting_clean() {
        for argv in [
            vec!["drep", "lint-docs"],
            vec!["drep", "doctor"],
            vec!["drep", "init"],
        ] {
            let cli = Cli::try_parse_from(argv).unwrap();
            assert!(run(cli).await.is_err());
        }
    }
}
