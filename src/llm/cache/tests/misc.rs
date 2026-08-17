//! Miscellaneous `Cache` tests.
//!
//! These do not correspond to one of the 24 numbered acceptance criteria
//! but are needed to pin behaviour cargo-mutants otherwise leaves
//! uncaught. `default_root` is a public function whose value depends on
//! the platform; a test that asserts a non-empty result catches the
//! "default = `PathBuf::default()`" mutation.

use crate::llm::cache::Cache;

/// `default_root` must return a real path, not the empty `PathBuf::default()`.
///
/// On macOS and Linux this is the platform cache dir (e.g.,
/// `~/Library/Caches/dev.slb350.drep`); on platforms without a cache dir
/// it falls back to `.drep-cache`. Both are non-empty. A mutant that
/// substitutes `Default::default()` returns the empty `PathBuf` and is
/// caught by the non-empty assertion below.
#[test]
fn default_root_returns_a_non_empty_path() {
    let root = Cache::default_root();
    assert!(
        !root.as_os_str().is_empty(),
        "default_root must return a real path, was empty"
    );
}
