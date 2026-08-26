//! Acceptance tests for `drep check`.
//!
//! The contracts are split by topic across sibling files: failure reporting,
//! input resolution, the deterministic layer, gating and exit codes, output
//! rendering, and tool resolution. The files are wired in here - if you add a
//! file, declare it. A file no `mod` declaration reaches is never compiled,
//! and cargo will not warn about it.

mod credentials;
mod deterministic;
mod failover_report;
mod failures;
mod gating;
mod input;
mod output;
mod resolution;
mod review_budget;
mod review_rounds;
mod site_policy;
pub(super) mod support;
mod unanalyzed_json;
