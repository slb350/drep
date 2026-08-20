//! Unit tests for `config`.
//!
//! Split by topic: the `[[llm]]` array shape, `${VAR}` expansion, and ordinary
//! field parsing. Every file here must be declared below — a file no `mod`
//! declaration reaches is never compiled, and cargo does not warn about it.

mod backends;
mod env;
mod fields;
mod providers;
mod support;
