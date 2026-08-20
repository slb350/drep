//! Unit tests for the interactive wizard.
//!
//! Wired in via `#[cfg(test)] mod tests;` in `wizard.rs`. Every file here must
//! be declared below: Rust silently ignores a file no `mod` points at, which
//! once left four files of tests uncompiled in this repository while the count
//! still looked right.

mod flow;
mod keys;
mod models;
mod support;

#[allow(unused_imports)]
pub(crate) use support::{Catalog, Recording};
pub(crate) use support::{Scripted, number_of};
