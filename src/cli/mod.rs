//! Command-line surface.
//!
//! Four commands, two triggers (pre-commit and pre-push). Anything that needs
//! a platform API, a webhook or a database was dropped in 2.0 - see
//! `docs/rust-migration.md`.
//!
//! Each command owns its arguments in its own module, so a command's contract
//! and its behaviour stay together as the later phases fill them in.

pub mod auth;
pub mod check;
pub mod doctor;
pub mod init;
pub mod lint_docs;
pub mod render;

use anyhow::Result;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand, ValueEnum};

use crate::analysis::findings::Severity;

use crate::Exit;
use auth::AuthArgs;
use check::CheckArgs;
use doctor::DoctorArgs;
use init::InitArgs;
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
    Doctor(DoctorArgs),
    /// Write the git hooks and LLM endpoint configuration.
    Init(InitArgs),
    /// Manage the API keys drep holds for this machine.
    Auth(AuthArgs),
}

/// Parse a `--fail-on` severity, for whichever command takes one.
///
/// Built from `Severity::ALL` rather than a literal list, so `--help` shows
/// exactly the values `FromStr` accepts and neither can drift. Shared by
/// `check` and `lint-docs`: two commands in one binary that disagree about the
/// severity vocabulary are two contracts a hook author has to learn.
pub(crate) fn severity_parser() -> impl TypedValueParser<Value = Severity> {
    PossibleValuesParser::new(Severity::ALL.map(Severity::as_str))
        .map(|name| name.parse::<Severity>().expect("possible values parse"))
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
        Command::LintDocs(args) => lint_docs::run(&args, std::path::Path::new(".")).await,
        Command::Doctor(args) => doctor::run(&args),
        Command::Init(args) => init::run(&args).await,
        Command::Auth(args) => auth::run(&args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::findings::Severity;
    use crate::config;
    use clap::CommandFactory;

    /// Parse a `check` invocation and hand back its arguments.
    fn check_args<const N: usize>(argv: [&str; N]) -> CheckArgs {
        let cli = Cli::try_parse_from(argv).expect("should parse");
        match cli.command {
            Command::Check(args) => args,
            other => panic!("expected check, got {other:?}"),
        }
    }

    /// Parse a `lint-docs` invocation and hand back its arguments.
    fn lint_docs_args<const N: usize>(argv: [&str; N]) -> LintDocsArgs {
        let cli = Cli::try_parse_from(argv).expect("should parse");
        match cli.command {
            Command::LintDocs(args) => args,
            other => panic!("expected lint-docs, got {other:?}"),
        }
    }

    #[test]
    fn lint_docs_gating_is_off_by_default() {
        let args = lint_docs_args(["drep", "lint-docs"]);
        assert!(!args.strict);
        assert_eq!(args.fail_on, None);
        assert_eq!(args.threshold(), None);
    }

    #[test]
    fn lint_docs_fail_on_accepts_the_whole_severity_vocabulary() {
        for expected in Severity::ALL {
            let args = lint_docs_args(["drep", "lint-docs", "--fail-on", expected.as_str()]);
            assert_eq!(args.fail_on, Some(expected));
            assert_eq!(args.threshold(), Some(expected));
        }
        assert!(Cli::try_parse_from(["drep", "lint-docs", "--fail-on", "critical"]).is_err());
    }

    /// `--strict` is the shorthand, not a second mechanism.
    #[test]
    fn lint_docs_strict_is_fail_on_info() {
        assert_eq!(
            lint_docs_args(["drep", "lint-docs", "--strict"]).threshold(),
            Some(Severity::Info)
        );
    }

    /// Passing both asks two different questions at once. Which wins is not a
    /// thing a user should have to remember, so clap refuses.
    #[test]
    fn lint_docs_strict_and_fail_on_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from(["drep", "lint-docs", "--strict", "--fail-on", "error"]).is_err()
        );
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
        assert!(!args.cache_only);
        assert!(!args.push_gate);
    }

    #[test]
    fn cache_only_and_push_gate_parse_individually_but_not_together() {
        assert!(check_args(["drep", "check", "--cache-only"]).cache_only);
        assert!(check_args(["drep", "check", "--push-gate"]).push_gate);
        assert!(Cli::try_parse_from(["drep", "check", "--cache-only", "--push-gate"]).is_err());
    }

    #[test]
    fn default_repository_config_path_is_relative_to_the_requested_root() {
        assert!(config::default_config_path().is_relative());
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

    #[test]
    fn every_command_dispatches_to_an_implementation() {
        // The last stub (`lint-docs`) landed in Phase 6, so there is no
        // `unimplemented` arm left to pin. What replaces that test is the
        // guarantee it was really protecting: no subcommand may reach `run`
        // and fall through to a clean exit. `run`'s match is exhaustive over
        // `Command`, so the compiler enforces it - this asserts the enum is
        // still the commands the contract names, so one added without an arm
        // is a compile error rather than a silent pass.
        let names: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_owned())
            .collect();
        assert_eq!(names, vec!["check", "lint-docs", "doctor", "init", "auth"]);
    }

    #[test]
    fn lint_docs_takes_paths_and_strict() {
        let cli = Cli::try_parse_from(["drep", "lint-docs", "--strict", "a.md"]).unwrap();
        match cli.command {
            Command::LintDocs(args) => {
                assert!(args.strict);
                assert_eq!(args.paths.len(), 1);
            }
            other => panic!("expected lint-docs, got {other:?}"),
        }
        let cli = Cli::try_parse_from(["drep", "lint-docs"]).unwrap();
        match cli.command {
            Command::LintDocs(args) => {
                assert!(!args.strict, "report-only is the default");
                assert!(args.paths.is_empty());
            }
            other => panic!("expected lint-docs, got {other:?}"),
        }
    }
}
