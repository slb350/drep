//! A1: Languages found reports each present language and uses registration
//! order, not first-seen.
//!
//! A directory containing `a.py`, `b.py` and `main.go` reports
//! `  Python: 2 file(s)` and `  Go: 1 file(s)` under `Languages found:`, and
//! the Python line appears **before** the Go line (registration order).

use crate::cli::doctor::DoctorArgs;

fn args(path: &std::path::Path) -> DoctorArgs {
    DoctorArgs {
        path: path.to_path_buf(),
        config: None,
    }
}

#[tokio::test]
async fn languages_found_lists_each_language_in_registration_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("a.py");
    std::fs::write(dir.path().join("b.py"), "y = 2\n").expect("b.py");
    std::fs::write(dir.path().join("main.go"), "package main\n").expect("main.go");

    let mut out = Vec::new();
    let exit = super::run_scoped(&mut out, &args(dir.path()), dir.path())
        .await
        .expect("run_to");
    assert_eq!(exit, crate::Exit::Clean);
    let rendered = String::from_utf8(out).expect("utf8");

    let py_idx = rendered
        .find("  Python: 2 file(s)")
        .expect("Python line must appear");
    let go_idx = rendered
        .find("  Go: 1 file(s)")
        .expect("Go line must appear");
    assert!(
        py_idx < go_idx,
        "Python must appear before Go (registration order); rendered:\n{rendered}"
    );
}
