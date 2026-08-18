//! The code-quality analyzer: render a payload, ask the LLM, turn the
//! response into findings — and never report an unanalyzed file as clean.
//!
//! The contract this type implements is the Phase 4b spec verbatim:
//!
//! 1. No language, no analysis. drep has no opinion on a file type it does
//!    not claim, and silently returning an empty result is the correct
//!    behavior — *not* a failure. The CLI surfaces "no language" by simply
//!    not including the file in the work set.
//! 2. Empty hunks → empty result, no LLM call.
//! 3. Build the payload with `payload::render`. `None` → empty result.
//! 4. Cache first. A hit is parsed exactly as a `Complete` response would
//!    be, and the duplicate is silent — the caller's view is identical.
//! 5. Concurrency. A limiter slot is acquired before the LLM call and held
//!    for the duration. A cache hit must not acquire a slot: the slot
//!    represents in-flight HTTP work, and a cache read is not in-flight.
//! 6. `Ok(Extracted::Complete)` → parse, store in the cache.
//! 7. `Ok(Extracted::Truncated)` → parse the partial result AND mark the
//!    file failed. Never cache a truncated response — caching it makes one
//!    truncation permanent for the whole TTL, and this layer does not know
//!    about `--fail-on` (a caller deciding otherwise would make
//!    `failed_files` depend on a CLI flag, which is the wrong layering).
//! 8. `Err(LlmError::*)` → no findings, file in `failed_files` with the
//!    specific LLM layer that failed.
//!
//! The five rules around out-of-range lines, missing fields, unknown
//! severities, and `issues` itself being absent are at the boundary between
//! "model misreported" and "we could not understand the response". The first
//! is a *finding* we drop; the others are *file-level failures*, because a
//! file we did not fully understand must never be reported clean.

use std::path::Path;

use futures::future::join_all;
use serde_json::Value;

use crate::analysis::findings::{Finding, LlmSeverity};
use crate::analysis::payload;
use crate::analysis::prompt::build_analysis_prompt;
use crate::analysis::result::{AnalysisResult, FailureReason};
use crate::config::LlmConfig;
use crate::diff::hunks::Hunk;
use crate::languages;
use crate::llm::cache::Cache;
use crate::llm::client::{LlmClient, LlmError};
use crate::llm::concurrency::Limiter;
use crate::llm::json_parsing::Extracted;

/// The code-quality analyzer.
///
/// Built once per process. The `cache` and the `limiter` are both passed in
/// rather than constructed here: they are process-wide resources, and a
/// limiter built per analyzer would stop capping the moment a second
/// analyzer existed - two of them would put `2 * max_concurrent` requests in
/// flight against one endpoint.
///
/// The model and temperature are **not** duplicated onto this struct. They
/// are the request parameters `LlmClient` already validated and owns, its
/// fields are `pub(crate)` and so readable from here, and a second copy is
/// exactly what lets a request go to one model while the cache key names
/// another.
pub struct CodeQualityAnalyzer {
    pub(crate) client: LlmClient,
    pub(crate) cache: Cache,
    pub(crate) limiter: Limiter,
}

impl CodeQualityAnalyzer {
    /// Build from config, a shared cache and a shared limiter.
    ///
    /// Both are parameters rather than constructed here so one process has
    /// one of each. `cache` additionally makes the key independent of the
    /// `Cache` root (see [`Cache::key`]).
    ///
    /// Returns the same [`LlmError::NotConfigured`] as `LlmClient::new`: a
    /// disabled or misconfigured LLM is fatal at construction so the gate
    /// fails fast rather than silently skipping analysis.
    pub fn new(cfg: &LlmConfig, cache: Cache, limiter: Limiter) -> Result<Self, LlmError> {
        Ok(Self {
            client: LlmClient::new(cfg)?,
            cache,
            limiter,
        })
    }

    /// Analyze one file's hunks.
    ///
    /// Returns an [`AnalysisResult`] populated with whatever the file
    /// produced: findings, a failure marker, or both. The result is never a
    /// bare `Vec<Finding>`; the failure axis is part of the return type so
    /// the caller cannot forget it.
    pub async fn analyze_file(&self, hunks: &[Hunk]) -> AnalysisResult {
        // Rule 1: no language, no analysis. `languages::detect` on the
        // first hunk's file path is enough because every hunk in `by_file`
        // shares a path (the diff module groups by file).
        let Some(first) = hunks.first() else {
            return AnalysisResult::default();
        };
        let Some(language) = languages::detect(&first.file_path) else {
            return AnalysisResult::default();
        };

        // Rule 3: payload. `render` returns `None` only for an empty slice,
        // which the `hunks.first()` guard above has already excluded - rule 2
        // and rule 3 are the same check, so a second `hunks.is_empty()` here
        // would be unreachable rather than defensive.
        let Some(payload) = payload::render(language, hunks) else {
            return AnalysisResult::default();
        };

        let system_prompt = build_analysis_prompt(language);
        let cache_key = self.cache.key(
            &system_prompt,
            &payload.text,
            &self.client.model,
            self.client.temperature,
        );

        // Rule 4: cache first. A hit is parsed exactly as a `Complete`
        // response would be, and the duplicate is silent — the caller's
        // view is identical to a fresh request.
        if let Some(cached) = self.cache.get(&cache_key) {
            return parse_response(&payload, &first.file_path, &Extracted::Complete(cached));
        }

        // Rule 5: concurrency. Acquire a slot before the LLM call and
        // hold it for the duration. A cache hit must not acquire a slot:
        // the slot represents in-flight HTTP work, and a cache read is
        // not in-flight.
        let _guard = self.limiter.acquire().await;
        let result = self
            .client
            .complete_json(&system_prompt, &payload.text)
            .await;

        match result {
            // Rule 6: complete → parse, store in the cache.
            // Rules 6 and 7 in one arm. Both never-cache rules live in the
            // `if let` below rather than being split across two arms, where
            // the `Complete` arm's guard could only ever be true.
            Ok(extracted) => {
                let result = parse_response(&payload, &first.file_path, &extracted);
                // Cache only a `Complete` response we fully understood. A
                // truncated one is a prefix, and a body can be valid JSON and
                // still schema-invalid - a missing `issues` array, a record
                // with an unknown severity - which yields a file-level
                // failure. Caching either replays it for the whole TTL
                // instead of letting the next run ask again.
                //
                // The write itself is best-effort: a cache failure is a
                // diagnostic, not a failure of the analysis.
                if let Extracted::Complete(value) = &extracted
                    && result.failed_files.is_empty()
                {
                    let _ = self.cache.put(&cache_key, value);
                }
                result
            }
            // Rule 8: any LLM error → no findings, file in `failed_files`
            // with the specific reason. The detail is kept rather than
            // discarded, so the CLI can render a line the user can act on.
            Err(err) => {
                let mut result = AnalysisResult::default();
                result
                    .failed_files
                    .insert(first.file_path.clone(), LlmError::into_failure_reason(err));
                result
            }
        }
    }

    /// Analyze many files concurrently, bounded by the limiter.
    ///
    /// Each entry of `by_file` is one file's hunks; the limiter bounds the
    /// in-flight requests, so we spawn them all and let it queue. The
    /// per-file results are merged with [`AnalysisResult::merge`].
    pub async fn analyze_files(&self, by_file: &[Vec<Hunk>]) -> AnalysisResult {
        let futures = by_file.iter().map(|hunks| self.analyze_file(hunks));
        let results = join_all(futures).await;
        let mut merged = AnalysisResult::default();
        for result in results {
            merged.merge(result);
        }
        merged
    }
}

/// Map an `LlmError` to the failure reason the caller carries in
/// `AnalysisResult::failed_files`.
///
/// Distinct from the parsing-path reasons because the LLM layer's failure
/// modes are a different axis: a parse failure is a content problem, a
/// transport failure is a connectivity problem. The shape is forced by the
/// spec — `FailureReason::Transport` keeps the HTTP status as a number — so
/// a caller can branch on `status` without re-parsing the message.
fn into_failure_reason(err: LlmError) -> FailureReason {
    match err {
        LlmError::Transport { status, message } => FailureReason::Transport { status, message },
        LlmError::Unparseable(message) => FailureReason::Unparseable(message),
        // `NotConfigured` is a configuration failure at the LLM boundary —
        // not a connectivity failure, but indistinguishable from one to the
        // gate, which only cares whether the file was analyzed. Mapping to
        // `Transport { status: None }` keeps the exit code 2 path uniform
        // without inventing a new variant the JSON output would have to
        // distinguish.
        LlmError::NotConfigured(message) => FailureReason::Transport {
            status: None,
            message,
        },
    }
}

impl LlmError {
    /// Translate into the [`FailureReason`] the analyzer carries.
    ///
    /// Public via the type so the CLI tests can pin the mapping without
    /// reaching for the analyzer's private function.
    pub fn into_failure_reason(self) -> FailureReason {
        into_failure_reason(self)
    }
}

/// Turn an `Extracted` value into an [`AnalysisResult`].
///
/// A free function, not a method: it reads no analyzer state, and keeping it
/// free means the whole parsing core is testable without a `MockServer`, a
/// `Cache` and a `TempDir`.
///
/// The truncation flag is **read off the discriminant**, never passed in
/// beside it. An earlier shape took `extracted` *and* a `truncated: bool`,
/// which let `(Extracted::Truncated(v), false)` compile and report a
/// truncated file as clean - the single outcome this module exists to
/// prevent, resting on four call sites agreeing by convention.
fn parse_response(
    payload: &payload::Payload,
    file_path: &Path,
    extracted: &Extracted,
) -> AnalysisResult {
    let mut result = AnalysisResult::default();

    let (value, truncated) = match extracted {
        Extracted::Complete(value) => (value, false),
        // Rule 7: a truncated response is a prefix of what the model meant,
        // so the file is unanalyzed however good the partial findings look.
        Extracted::Truncated(value) => (value, true),
    };

    // Hoisted: every failure path below names the same file, and every
    // finding carries the same path string.
    let path_string = file_path.to_string_lossy().into_owned();
    let mut malformed_reason: Option<String> = if truncated {
        Some("response was truncated".to_owned())
    } else {
        None
    };

    // The response shape is `{"issues": [...], "summary": "..."}`. Anything
    // else is malformed, and the whole file is unanalyzed.
    let Some(issues) = value.get("issues").and_then(Value::as_array) else {
        result.failed_files.insert(
            file_path.to_path_buf(),
            FailureReason::MalformedFinding("response has no `issues` array".to_owned()),
        );
        return result;
    };

    // `issues: []` is a legitimate clean result: empty findings, no failure.
    for issue in issues {
        match parse_issue(issue, payload, &path_string) {
            IssueOutcome::Finding(finding) => result.findings.push(finding),
            IssueOutcome::Dropped => result.dropped_out_of_range += 1,
            // Do not return early: a malformed record in the middle of an
            // otherwise-valid array should still let the well-formed records
            // through. The failure class is "we do not fully understand the
            // response", not "every record is wrong".
            IssueOutcome::Malformed(detail) => malformed_reason = Some(detail),
        }
    }

    if let Some(detail) = malformed_reason {
        let reason = if truncated {
            FailureReason::Truncated
        } else {
            FailureReason::MalformedFinding(detail)
        };
        result.failed_files.insert(file_path.to_path_buf(), reason);
    }
    result
}

/// Parse one issue record into a [`Finding`], a drop, or a malformed
/// marker with a reason.
///
/// The three outcomes are deliberately distinct:
///
/// - `Finding`: a valid record attributed to a real line in the
///   payload. The caller adds it to `findings`.
/// - `Dropped`: a valid record whose line was not in
///   `payload.valid_lines`. The caller increments
///   `dropped_out_of_range`. The file is **not** marked failed: we
///   understood the record perfectly, it was simply about code the
///   model was never shown.
/// - `Malformed`: an unparseable record (unknown severity, missing
///   field, non-integer `line`). The caller adds the file to
///   `failed_files`. We cannot know what the record meant, so we
///   cannot trust the rest of the response either.
fn parse_issue(issue: &Value, payload: &payload::Payload, file_path: &str) -> IssueOutcome {
    // `line` must be a positive integer. A missing field, a string,
    // a float, or a non-positive number are all malformed.
    let Some(line) = issue.get("line").and_then(Value::as_u64) else {
        return IssueOutcome::Malformed("missing or non-integer `line`".to_owned());
    };
    // `Value::as_u64` already rejects non-integers; the only
    // remaining "not a positive integer" case is zero. A line of
    // zero is a model artifact, not a real line.
    if line == 0 {
        return IssueOutcome::Malformed("`line` is zero".to_owned());
    }
    // A line beyond `u32` is a model artifact of exactly the same class as
    // a line of zero, and this module's whole thesis is that we do not guess.
    // Clamping to `u32::MAX` happened to land in `Dropped` because that value
    // is never in `valid_lines` - correct by accident, via a silent clamp.
    let Ok(line) = u32::try_from(line) else {
        return IssueOutcome::Malformed("`line` is beyond u32".to_owned());
    };

    // `severity` must be one of the levels the prompt asked for. Anything
    // else is malformed: we cannot map it to a `Severity`, and we do not
    // silently coerce. The vocabulary lives on `LlmSeverity` so the prompt
    // and this parser cannot list different levels.
    let Some(severity_str) = issue.get("severity").and_then(Value::as_str) else {
        return IssueOutcome::Malformed("missing `severity`".to_owned());
    };
    let Ok(severity) = severity_str.parse::<LlmSeverity>() else {
        return IssueOutcome::Malformed(format!("unknown severity `{severity_str}`"));
    };
    let severity = severity.to_severity();

    // `message` is the only remaining required field. Missing or
    // non-string is malformed.
    let Some(message) = issue.get("message").and_then(Value::as_str) else {
        return IssueOutcome::Malformed("missing or non-string `message`".to_owned());
    };

    let kind = issue
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let message = message.to_owned();

    // `suggestion` is optional. Absent or empty → `None`, not
    // `Some("")`: an empty suggestion is not a suggestion.
    let suggestion = issue
        .get("suggestion")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // Membership is checked LAST, after the record's shape.
    //
    // Shape asks "did the model answer in our schema"; membership asks "did it
    // talk about code we sent". A record with an unknown severity is evidence
    // the response's vocabulary is wrong, and that contaminates the records we
    // did accept - so it must fail the file even when the record also cites a
    // line we never sent. Checking membership first would let a demonstrably
    // schema-violating response be reported as fully understood.
    //
    // A well-formed record about code we did not send is different: we
    // understood it perfectly, it is simply out of scope. It is dropped and
    // counted, never clamped onto the nearest valid line, which would attach a
    // real-looking finding to arbitrary code. Pinned by
    // `out_of_range_line_is_dropped_not_clamped`.
    if !payload.valid_lines.contains(&line) {
        return IssueOutcome::Dropped;
    }

    IssueOutcome::Finding(Finding {
        kind,
        severity,
        file_path: file_path.to_owned(),
        line,
        column: None,
        message,
        suggestion,
    })
}

/// What one issue record became after parsing.
enum IssueOutcome {
    /// A valid record attributed to a real line in the payload.
    Finding(Finding),
    /// A valid record whose line was not in `payload.valid_lines`.
    Dropped,
    /// An unparseable record, with a reason naming what was wrong.
    Malformed(String),
}
