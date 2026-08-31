//! Unit tests for `config`.
//!
//! Split by topic: the `[[llm]]` array shape, `${VAR}` expansion, credential
//! declaration, ordinary field parsing, and the machine-level site policy layer.
//! Every file here must be declared below — a file no `mod` declaration reaches
//! is never compiled, and cargo does not warn about it.

mod backends;
mod credentials;
mod env;
mod fields;
mod headers;
mod providers;
mod site;
mod support;
