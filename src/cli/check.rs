//! `drep check` - the commit gate.
//!
//! Phase 5 adds the implementation; Phase 0 fixes the argument contract.

use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{ArgGroup, Args};

use crate::analysis::findings::Severity;
use crate::cli::OutputFormat;

#[derive(Debug, Args)]
// One rule, stated once. Paired `conflicts_with_all` attributes say the same
// thing from each side and have to be kept in agreement; a fourth input mode
// would mean editing every existing one, and missing a single edit silently
// permits an illegal combination.
#[command(group(ArgGroup::new("input").args(["paths", "staged", "diff"]).multiple(false)))]
pub struct CheckArgs {
    /// Files or directories to check. Duplicates and overlaps are collapsed,
    /// so `drep check a.rs .` analyzes `a.rs` once.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Check the files staged for commit. For a pre-commit hook.
    #[arg(long)]
    pub staged: bool,

    /// Check the files changed since REF, e.g. `origin/main`. For pre-push.
    #[arg(long, value_name = "REF")]
    pub diff: Option<String>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Also block on LLM findings at or above this severity.
    ///
    /// Deterministic tool findings always block; this opts the LLM's findings
    /// into gating too. Left unset, they inform without blocking - which is
    /// the useful default, because the model emits style suggestions on
    /// nearly every file.
    #[arg(long, value_name = "SEVERITY", value_parser = severity_parser())]
    pub fail_on: Option<Severity>,
}

/// Parse `--fail-on` from the severity vocabulary.
///
/// Built from `Severity::ALL` rather than a literal list, so `--help` shows
/// exactly the values `FromStr` accepts and neither can drift.
fn severity_parser() -> impl TypedValueParser<Value = Severity> {
    PossibleValuesParser::new(Severity::ALL.map(Severity::as_str))
        .map(|name| name.parse::<Severity>().expect("possible values parse"))
}
