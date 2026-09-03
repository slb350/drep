//! The Rust ecosystem: clippy over `.rs`.

use crate::languages::spec::{LanguageSupport, ToolSpec};

/// Rust linter - emits structured JSON via cargo's message-format.
pub static CLIPPY: ToolSpec = ToolSpec {
    name: "clippy",
    command: &["cargo", "clippy", "--message-format", "json", "--quiet"],
    local_paths: &[],
    config_files: &["Cargo.toml"],
    config_flag: None,
    output_format: "cargo",
    diagnostics_stream: "stdout",
    // Cargo's build lock is acquired by Cargo itself, so its wait is part of
    // the child process. Allow the same long-running ceiling as an LLM review
    // rather than failing a whole gate at the generic two-minute tool limit.
    timeout_secs: 1_800,
    timeout_context: Some(", including its Cargo build-lock wait"),
    establishes_compilation: true,
    serial_in_repository: true,
    // `cargo clippy` checks a crate, not files: a path argument is rejected
    // with "unexpected argument". See `ToolSpec::accepts_files`.
    accepts_files: false,
};

/// Rust language entry.
pub static RUST_LANG: LanguageSupport = LanguageSupport {
    name: "rust",
    display_name: "Rust",
    extensions: &[".rs"],
    filenames: &[],
    tools: &[&CLIPPY],
    conventions: &[
        "unwrap/expect on values that can legitimately be None or Err",
        "unsafe blocks, and whether their invariants are documented",
        "Unnecessary clones and allocations in hot paths",
        "Send/Sync correctness for types crossing threads",
    ],
    vendored_dirs: &["target"],
};
