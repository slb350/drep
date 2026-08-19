//! Analysis-specific test fixtures.
//!
//! The mock-endpoint helpers (`cfg_for`, `sse`, `request_count`,
//! `fast_retry_client`, `server_returning`) live in `crate::test_support` and
//! are shared with the LLM client suite. Only the analyzer builder is specific
//! to this module.

use std::path::PathBuf;

use tempfile::TempDir;
use wiremock::MockServer;

use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::config::LlmConfig;
use crate::diff::hunks::{Hunk, HunkLine};
use crate::llm::cache::Cache;
use crate::llm::chain::ProviderChain;
use crate::test_support::{cfg_for, fast_retry_chain, temp_cache};

/// Build a single-provider analyzer whose backoff delays are shrunk.
///
/// Going through the production constructors is the point. An earlier version
/// assembled the struct by literal, which meant it restated the cache-key
/// derivation by hand - so a change to how the key is built would leave these
/// tests passing against a key production never generates. The per-provider
/// limiter comes from `cfg.max_concurrent` exactly as it does in production, so
/// a test wanting serial execution sets that field rather than reaching for a
/// second constructor.
pub(crate) fn analyzer_with_fast_retry(cfg: &LlmConfig, cache: Cache) -> CodeQualityAnalyzer {
    CodeQualityAnalyzer::new(fast_retry_chain(std::slice::from_ref(cfg)), cache)
}

/// A `Hunk` carrying one Python file with one added line at a known position.
/// The line number is what the mocked response should reference.
pub(crate) fn python_hunk(file_path: &str, line_no: u32) -> Hunk {
    Hunk {
        file_path: PathBuf::from(file_path),
        // A pure insertion: the old side contributes no lines, so `old_count`
        // is 0. It was 1, which describes a one-line modification and does not
        // match a `lines` vector holding only `Added` entries.
        old_start: line_no.saturating_sub(1),
        old_count: 0,
        new_start: line_no,
        new_count: 1,
        lines: vec![HunkLine::Added("x = 1".to_owned())],
    }
}

/// A one-line Python file at `line_no`.
pub(crate) fn hunks_for_python_at(line_no: u32) -> Vec<Hunk> {
    vec![python_hunk("src/lib.py", line_no)]
}

/// A two-line Python file: the response should reference lines 100-101.
pub(crate) fn hunks_for_python_at_two_lines() -> Vec<Hunk> {
    vec![Hunk {
        file_path: PathBuf::from("src/lib.py"),
        old_start: 99,
        // Pure insertion again - see `python_hunk`.
        old_count: 0,
        new_start: 100,
        new_count: 2,
        lines: vec![
            HunkLine::Added("a = 1".to_owned()),
            HunkLine::Added("b = 2".to_owned()),
        ],
    }]
}

/// Build an analyzer backed by `server` and a fresh cache directory.
///
/// The `TempDir` is returned so the caller keeps it alive: dropping it would
/// delete the cache mid-test.
pub(crate) fn analyzer_for(server: &MockServer) -> (CodeQualityAnalyzer, TempDir) {
    let (cache, dir) = temp_cache();
    let cfg = cfg_for(server, "m", 3);
    let chain = ProviderChain::new(&[&cfg]).expect("chain builds from a valid config");
    let analyzer = CodeQualityAnalyzer::new(chain, cache);
    (analyzer, dir)
}
