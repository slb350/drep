//! Unit tests for the provider chain.
//!
//! Split by the question each file answers. Every file here must be declared
//! below - a Rust file no `mod` declaration reaches is never compiled, and a
//! test file that is never compiled looks exactly like a passing one.
//!
//! Two things these tests deliberately do NOT try to demonstrate:
//!
//! - **Intermittency.** wiremock returns exactly what it is told, so no test
//!   here can show a provider that fails *sometimes*. That is the shape of
//!   every interesting failover case in production, and it is why the retry
//!   classification is pinned by the deterministic cases instead.
//! - **That the loop merely advances.** A dead provider A and a healthy
//!   provider B prove the loop moves; they say nothing about whether the cache
//!   key moved with it. `cache_key.rs` is the file that matters.

mod cache_key;
mod construction;
mod demotion;
mod failover;
mod support;
