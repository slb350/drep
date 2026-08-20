//! Unit tests for the JSON extraction ladder.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `json_parsing.rs`. Every file in
//! this directory must be declared here: Rust silently ignores a file no `mod`
//! points at, which once left four files of tests uncompiled in this repository
//! while the count still looked right.

mod balance;
mod fence;
mod ladder;
mod reasoning;
mod repair;
