//! The finding vocabulary.
//!
//! Phase 4 adds `Finding` itself. `Severity` lands here in Phase 0 because
//! `drep check --fail-on` is part of the CLI contract and needs it to parse.
//!
//! Deliberately free of `clap`: this module becomes the core analysis library,
//! and the tool parsers (Phase 1) and LLM response parsing (Phases 3-4) need
//! `Severity` without dragging an argument parser in behind it. The CLI adapts
//! to `FromStr` at its own boundary.

use std::str::FromStr;

/// Finding severity, lowest first.
///
/// The single vocabulary for a finding's severity. Producers map their own
/// scales onto it; consumers that gate on severity compare `Severity` values
/// directly rather than inventing a ranking.
///
/// Ordering is derived from declaration order, so it cannot drift from a
/// separate rank table and there is no "unknown severity" case to default.
/// In the Python implementation this was a `SEVERITY_RANK` dict that callers
/// had to remember to index rather than `.get(..., 0)` - a lookup with a
/// default silently passes a gate on a severity nobody ranked. Here the type
/// system removes the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Raised when a producer emits a severity outside the vocabulary.
///
/// An unrecognised severity is a bug to surface, not a value to coerce to the
/// lowest rank - coercion is how a finding silently stops blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSeverity(pub String);

impl std::fmt::Display for UnknownSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown severity `{}` (expected one of: ", self.0)?;
        for (i, sev) in Severity::ALL.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            f.write_str(sev.as_str())?;
        }
        f.write_str(")")
    }
}

impl std::error::Error for UnknownSeverity {}

impl Severity {
    /// Every severity, lowest first. The one place the vocabulary is listed.
    pub const ALL: [Severity; 3] = [Severity::Info, Severity::Warning, Severity::Error];

    /// The wire name, as tool parsers and the LLM emit it.
    ///
    /// `FromStr` is defined in terms of this, so the two directions cannot
    /// disagree.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl FromStr for Severity {
    type Err = UnknownSeverity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Severity::ALL
            .into_iter()
            .find(|sev| sev.as_str() == s)
            .ok_or_else(|| UnknownSeverity(s.to_owned()))
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_lowest_first() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn all_is_in_rank_order_and_complete() {
        // Guards the invariant that `ALL` and the derived `Ord` agree; a
        // variant added out of order would make `ALL` a second, wrong ranking.
        assert!(Severity::ALL.is_sorted());
    }

    #[test]
    fn gating_at_error_admits_only_error() {
        let threshold = Severity::Error;
        assert!(Severity::Error >= threshold);
        assert!(Severity::Warning < threshold);
        assert!(Severity::Info < threshold);
    }

    #[test]
    fn wire_names_round_trip() {
        for sev in Severity::ALL {
            assert_eq!(sev.as_str().parse::<Severity>(), Ok(sev));
            assert_eq!(sev.to_string(), sev.as_str());
        }
    }

    #[test]
    fn unknown_severity_is_an_error_not_a_default() {
        let err = "critical".parse::<Severity>().unwrap_err();
        assert_eq!(err, UnknownSeverity("critical".to_owned()));
        assert!(err.to_string().contains("critical"));
        // The message must list the vocabulary, so a producer mismatch is
        // diagnosable from the error alone.
        assert!(err.to_string().contains("info, warning, error"));
    }

    #[test]
    fn parsing_is_case_sensitive() {
        // Producers emit lowercase. Accepting "ERROR" would mean quietly
        // normalising, and normalising is how a second vocabulary starts.
        assert!("ERROR".parse::<Severity>().is_err());
    }
}
