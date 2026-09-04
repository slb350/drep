//! Tests for the language registry.
//!
//! Every file in this directory must be declared below; a file no `mod`
//! points at is never compiled, which is how 31 tests once silently did not
//! run. `detect.rs` covers path resolution; `registry.rs` covers the
//! contracts every registered entry must hold.

mod detect;
mod registry;
