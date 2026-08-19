//! Unit tests for `Cache`.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `cache.rs`. These files were
//! orphaned once - present on disk but reachable by no `mod` declaration,
//! so cargo never compiled them and appending invalid Rust did not fail
//! the build. If you add a file here, declare it in this directory's
//! `mod.rs`.
//!
//! Tests are split one file per spec section (key, get/put, ttl, eviction,
//! misc) to keep individual files small and to mirror the spec's
//! organisation.

mod eviction;
mod get_put;
mod key;
mod misc;
mod ttl;
