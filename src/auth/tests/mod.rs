//! Unit tests for the credential store.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `auth.rs`. Every file here must be
//! declared below: Rust silently ignores a file no `mod` points at, which once
//! left four files of tests uncompiled in this repository while the count still
//! looked right.

mod resolve;
mod store;
