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

/// One finding produced by an analyzer.
///
/// Field naming follows the Python `drep.models.findings.Finding` so the two
/// stay translatable; `kind` here is `type` there, since `type` is a reserved
/// word in Rust and would force `r#type` at every call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Rule code (e.g. `"F401"`) for a structured finding, or the tool name
    /// (`"ruff"`, `"gofmt"`) when the tool emits no per-rule identifier.
    pub kind: String,
    pub severity: Severity,
    pub file_path: String,
    pub line: u32,
    pub column: Option<u32>,
    pub message: String,
    /// Optional one-line suggested fix from the tool. None when the tool
    /// does not emit one (e.g. eslint messages carry only a rule id).
    pub suggestion: Option<String>,
}

/// The five-level scale the LLM is asked to use, and its mapping onto
/// [`Severity`].
///
/// The model does not emit `Severity` directly: a reviewer reasons in
/// critical/high/medium/low/info, and collapsing that to three levels is
/// drep's decision, not the model's. Keeping the wire vocabulary as its own
/// type means the prompt renders the alternation from `ALL` and the parser
/// accepts exactly the same list, so the two cannot drift. They previously
/// could, and the consequence was not cosmetic: a level named in the prompt
/// but missing from the parser makes every record carrying it `Malformed`,
/// which marks the file unanalyzed and turns the gate's exit code to 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl LlmSeverity {
    /// Every level, most severe first - the order the prompt lists them in.
    pub const ALL: [LlmSeverity; 5] = [
        LlmSeverity::Critical,
        LlmSeverity::High,
        LlmSeverity::Medium,
        LlmSeverity::Low,
        LlmSeverity::Info,
    ];

    /// The wire name, as the prompt asks for it and the response carries it.
    pub const fn as_str(self) -> &'static str {
        match self {
            LlmSeverity::Critical => "critical",
            LlmSeverity::High => "high",
            LlmSeverity::Medium => "medium",
            LlmSeverity::Low => "low",
            LlmSeverity::Info => "info",
        }
    }

    /// Collapse onto drep's three-level vocabulary.
    pub const fn to_severity(self) -> Severity {
        match self {
            LlmSeverity::Critical | LlmSeverity::High => Severity::Error,
            LlmSeverity::Medium => Severity::Warning,
            LlmSeverity::Low | LlmSeverity::Info => Severity::Info,
        }
    }

    /// The `critical|high|medium|low|info` alternation, for the prompt.
    ///
    /// Rendered from `ALL` rather than written out, so a level added here
    /// reaches the prompt without anyone remembering to update it.
    pub fn alternation() -> String {
        Self::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("|")
    }
}

impl FromStr for LlmSeverity {
    type Err = UnknownSeverity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LlmSeverity::ALL
            .into_iter()
            .find(|level| level.as_str() == s)
            .ok_or_else(|| UnknownSeverity(s.to_owned()))
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

    #[test]
    fn llm_severity_wire_names_round_trip() {
        for level in LlmSeverity::ALL {
            assert_eq!(level.as_str().parse::<LlmSeverity>(), Ok(level));
        }
        assert!("blocker".parse::<LlmSeverity>().is_err());
    }

    #[test]
    fn llm_severity_collapses_onto_the_three_level_vocabulary() {
        // All five in one assertion: a single hardcoded mapping cannot pass.
        let mapped: Vec<Severity> = LlmSeverity::ALL
            .into_iter()
            .map(LlmSeverity::to_severity)
            .collect();
        assert_eq!(
            mapped,
            vec![
                Severity::Error,
                Severity::Error,
                Severity::Warning,
                Severity::Info,
                Severity::Info
            ]
        );
    }

    #[test]
    fn alternation_is_derived_from_all_not_written_out() {
        assert_eq!(LlmSeverity::alternation(), "critical|high|medium|low|info");
        // Every level must appear, so adding one cannot silently miss the prompt.
        for level in LlmSeverity::ALL {
            assert!(LlmSeverity::alternation().contains(level.as_str()));
        }
    }
}
