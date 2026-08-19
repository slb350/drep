//! Unit tests for `Limiter`.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `concurrency.rs`. Same orphan
//! caveat as `cache/tests/mod.rs`: a file under this directory that is not
//! declared here will not be compiled by `cargo test`, and appending
//! invalid Rust will not fail the build.

mod limiter;
