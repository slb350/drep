//! Tool resolution: criterion 24.
//!
//! The PATH lookup the deterministic runner uses when the project's own
//! copy of a tool is absent must refuse to return a `PATH` entry that
//! exists but is not executable - half-installed shims that the OS
//! would refuse to run otherwise slip through and crash a moment later
//! in `Command::spawn`, where the failure is harder to attribute.
//!
//! Both halves of the contract are pinned in one test so a resolver
//! that always returns `None` cannot pass it: the file is non-executable
//! at first (must miss), then executable (must hit, at the same path).
//!
//! `which_first` itself is private to `languages::runner`. The test goes
//! through `resolve_tool` with an empty `local_paths` list, which makes
//! the second branch the only path that can return a value - so what
//! this test pins is exactly the `which_first` behaviour the spec wants.

use std::path::Path;

use crate::languages::runner::resolve_tool;
use crate::languages::spec::ToolSpec;
use crate::test_support::{PathGuard, make_executable};

/// Criterion 24: the PATH lookup rejects a non-executable PATH entry
/// and accepts the same file once the executable bit is set.
///
/// Both halves in one test. A resolver that always returns `None` would
/// fail the second assertion; a resolver that ignores the executable
/// bit would fail the first.
#[test]
fn path_lookup_skips_non_executable_then_returns_it_when_executable() {
    let _guard = crate::test_support::lock_path();

    let dir = tempfile::tempdir().expect("tempdir");
    let tool = dir.path().join("mytool");
    std::fs::write(&tool, "#!/bin/sh\necho ok\n").expect("write tool");

    // A spec with empty `local_paths` makes the second branch the only
    // one that can return - which is the branch the criterion pins.
    let spec = ToolSpec {
        name: "mytool",
        command: &["mytool"],
        local_paths: &[],
        ..ToolSpec::default()
    };

    // With no executable bit, the file exists on PATH but the lookup
    // must skip it. Using a fresh, isolated PATH keeps the test from
    // accidentally hitting a real `mytool` shipped by the system.
    let path_guard = PathGuard::set(dir.path());
    assert!(
        resolve_tool(&spec, Path::new("/nonexistent-root-for-test")).is_none(),
        "a non-executable PATH entry must be skipped"
    );

    make_executable(&tool);
    let resolved = resolve_tool(&spec, Path::new("/nonexistent-root-for-test"));
    assert_eq!(
        resolved.as_deref(),
        Some(tool.as_path()),
        "after chmod +x, the lookup must return the same file"
    );

    // Drop `path_guard` first to restore PATH, then release the mutex.
    drop(path_guard);
    drop(_guard);
}
