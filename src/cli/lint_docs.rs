//! `drep lint-docs` - rule-based markdown checks. No LLM, no network.
//!
//! Phase 6 adds the implementation; Phase 0 fixes the argument contract.

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
pub struct LintDocsArgs {
    /// Markdown files or directories. Defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Exit non-zero when a check fires. Report-only by default.
    #[arg(long)]
    pub strict: bool,
}
