//! drep - a local commit gate.
//!
//! Two layers, split by *source* rather than severity:
//!
//! - **Deterministic**: the linters and formatters the repository has already
//!   configured (ruff, eslint, tsc, gofmt, go vet, clippy). Precise enough to
//!   block a commit.
//! - **Semantic**: an LLM, told which language it is reading. It informs
//!   unless `--fail-on` opts it into gating.
//!
//! Splitting by source is what makes the gate calibratable. Severity
//! thresholds over LLM output never were.

pub mod analysis;
pub mod cli;
pub mod config;
pub mod diff;
pub mod files;
pub mod languages;
pub mod llm;

/// Shared fixtures for tests across every module. Compiled only under
/// `cfg(test)`, so it adds nothing to the shipped binary.
#[cfg(test)]
pub(crate) mod test_support;

/// How the process terminated.
///
/// Load-bearing, not cosmetic: a gate that cannot tell "clean" apart from
/// "could not analyze" green-lights a commit whenever the LLM endpoint is
/// unreachable, which is worse than having no gate at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Analysis completed and found nothing at or above the gating threshold.
    Clean,
    /// Analysis completed and found issues that block.
    FoundIssues,
    /// One or more files could not be analyzed. Never reported as clean.
    Unanalyzed,
}

impl Exit {
    /// The process exit status.
    ///
    /// Hook scripts and CI branch on these numbers, so they are public API.
    ///
    /// Note that clap exits 2 on a usage error without passing through this
    /// type, so a caller seeing 2 cannot tell a bad flag from a failed
    /// analysis. That collision is deliberate and safe: both mean "do not let
    /// this commit through". The distinction to protect is 0 from everything
    /// else, not 1 from 2.
    pub const fn code(self) -> u8 {
        match self {
            Exit::Clean => 0,
            Exit::FoundIssues => 1,
            Exit::Unanalyzed => 2,
        }
    }
}

impl From<Exit> for std::process::ExitCode {
    fn from(exit: Exit) -> Self {
        std::process::ExitCode::from(exit.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(Exit::Clean.code(), 0);
        assert_eq!(Exit::FoundIssues.code(), 1);
        assert_eq!(Exit::Unanalyzed.code(), 2);
    }
}
