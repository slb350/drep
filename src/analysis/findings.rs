//! The finding vocabulary.
//!
//! Deliberately free of `clap`: this module becomes the core analysis library,
//! and both tool parsers and LLM response parsing need `Severity` without
//! dragging an argument parser in behind it. The CLI adapts to `FromStr` at its
//! own boundary.

use std::str::FromStr;

/// Finding severity, lowest first.
///
/// The single vocabulary for a finding's severity. Producers map their own
/// scales onto it; consumers that gate on severity compare `Severity` values
/// directly rather than inventing a ranking.
///
/// Ordering is derived from declaration order, so it cannot drift from a
/// separate rank table and there is no "unknown severity" case to default. A
/// lookup with a default could silently pass a gate on a severity nobody
/// ranked; the type system removes that possibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Does any finding sit at or above `threshold`?
///
/// The one definition of "this finding blocks". `check` and `lint-docs` both
/// gate on it and the `lint-docs` footer reports on it, and the comparison was
/// written out at all three sites - the same drift `SEVERITY_RANK` living here
/// exists to prevent, one level up.
pub fn any_at_or_above(findings: &[Finding], threshold: Severity) -> bool {
    findings.iter().any(|finding| finding.severity >= threshold)
}

/// Raised when a producer emits a severity outside the vocabulary.
///
/// An unrecognised severity is a bug to surface, not a value to coerce to the
/// lowest rank - coercion is how a finding silently stops blocking.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown severity `{value}` (expected one of: {})", expected.join(", "))]
pub struct UnknownSeverity {
    /// The value that failed to parse.
    pub value: String,
    /// The vocabulary that was expected.
    ///
    /// Carried rather than hardcoded because two different scales parse into
    /// this error: drep's three-level [`Severity`] and the model-facing
    /// five-level [`LlmSeverity`]. A fixed list meant a rejected `"blocker"`
    /// from an LLM response reported "expected one of: info, warning, error" -
    /// a vocabulary the parser does not accept and the model was never asked
    /// for, which sends whoever reads it looking in the wrong place.
    pub expected: &'static [&'static str],
}

impl Severity {
    /// Every severity, lowest first. The one place the vocabulary is listed.
    pub const ALL: [Severity; 3] = [Severity::Info, Severity::Warning, Severity::Error];

    /// Every wire name, in rank order — the list an error message quotes.
    ///
    /// Derived from `ALL` in a const, not written out. A second literal list
    /// would need a test to stop it drifting, and a derived list plus a
    /// consistency test is a weaker construction than derivation.
    pub const NAMES: [&'static str; 3] = [
        Self::ALL[0].as_str(),
        Self::ALL[1].as_str(),
        Self::ALL[2].as_str(),
    ];

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
            .ok_or_else(|| UnknownSeverity {
                value: s.to_owned(),
                expected: &Severity::NAMES,
            })
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding produced by an analyzer.
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
    /// Whether the LLM explicitly claims the code cannot compile. Tool
    /// findings and older cached responses leave this false.
    pub asserts_compile_failure: bool,
    /// Stable acknowledgement key for an LLM finding, when source context was
    /// available. Deterministic findings do not use acknowledgements.
    pub fingerprint: Option<String>,
}

impl Finding {
    /// Construct a rule-based finding, centralizing metadata that belongs only
    /// to semantic review.
    pub fn deterministic(
        kind: String,
        severity: Severity,
        file_path: String,
        line: u32,
        column: Option<u32>,
        message: String,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            kind,
            severity,
            file_path,
            line,
            column,
            message,
            suggestion,
            asserts_compile_failure: false,
            fingerprint: None,
        }
    }
}

/// The LLM severity vocabulary and its mapping onto [`Severity`].
///
/// New reviews request only critical/high/medium findings. The parser retains
/// low/info for compatibility with cached and unconstrained provider responses;
/// a recognized legacy level must not make the entire file `Malformed`.
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

    /// The material levels a new review is allowed to emit.
    pub const REVIEW: [LlmSeverity; 3] = [
        LlmSeverity::Critical,
        LlmSeverity::High,
        LlmSeverity::Medium,
    ];

    /// Every wire name, most severe first. Derived from `ALL` — see
    /// [`Severity::NAMES`].
    pub const NAMES: [&'static str; 5] = [
        Self::ALL[0].as_str(),
        Self::ALL[1].as_str(),
        Self::ALL[2].as_str(),
        Self::ALL[3].as_str(),
        Self::ALL[4].as_str(),
    ];

    /// Wire names exposed by the prompt and strict output schema.
    pub const REVIEW_NAMES: [&'static str; 3] = [
        Self::REVIEW[0].as_str(),
        Self::REVIEW[1].as_str(),
        Self::REVIEW[2].as_str(),
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

    /// The `critical|high|medium` alternation exposed by the prompt.
    pub fn review_alternation() -> String {
        Self::REVIEW_NAMES.join("|")
    }
}

impl FromStr for LlmSeverity {
    type Err = UnknownSeverity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LlmSeverity::ALL
            .into_iter()
            .find(|level| level.as_str() == s)
            .ok_or_else(|| UnknownSeverity {
                value: s.to_owned(),
                expected: &LlmSeverity::NAMES,
            })
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
    fn wire_names_round_trip() {
        for sev in Severity::ALL {
            assert_eq!(sev.as_str().parse::<Severity>(), Ok(sev));
            assert_eq!(sev.to_string(), sev.as_str());
        }
    }

    #[test]
    fn unknown_severity_is_an_error_not_a_default() {
        let err = "critical".parse::<Severity>().unwrap_err();
        assert_eq!(err.value, "critical");
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
    fn a_rejected_llm_severity_quotes_the_llm_vocabulary_not_dreps() {
        // The two scales share one error type. Reporting drep's three levels
        // for a rejected LLM level sends the reader looking for a value the
        // parser never accepts.
        let err = "blocker".parse::<LlmSeverity>().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("critical, high, medium, low, info"),
            "got {msg}"
        );
        assert!(
            !msg.contains("warning"),
            "must not quote drep's scale: {msg}"
        );

        let err = "blocker".parse::<Severity>().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("info, warning, error"), "got {msg}");
        assert!(
            !msg.contains("critical"),
            "must not quote the LLM scale: {msg}"
        );
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
    fn review_vocabulary_excludes_advisory_levels() {
        assert_eq!(LlmSeverity::review_alternation(), "critical|high|medium");
        assert_eq!(LlmSeverity::REVIEW_NAMES, ["critical", "high", "medium"]);
    }
}
