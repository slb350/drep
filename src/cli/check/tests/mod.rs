//! Acceptance tests for `drep check`.
//!
//! The 24 criteria the Phase 5a spec lists are split by topic across sibling
//! files: failure reporting, input resolution, the deterministic layer,
//! gating and exit codes, output rendering, and tool resolution. The files
//! are wired in here - if you add a file, declare it. A file no `mod`
//! declaration reaches is never compiled, and cargo will not warn about it.

mod deterministic;
mod failover_report;
mod failures;
mod gating;
mod input;
mod output;
mod resolution;
pub(super) mod support;
mod unanalyzed_json;
