//! Analysis results and the vocabulary they are reported in.
//!
//! Types live at one public path each - no facade re-exports, so consumers
//! cannot pick arbitrarily between `analysis::Severity` and
//! `analysis::findings::Severity` and drift apart.

pub mod findings;
pub mod payload;

#[cfg(test)]
mod tests;
