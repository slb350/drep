//! `result::AnalysisResult` — criteria 5-7.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::analysis::findings::{Finding, Severity};
use crate::analysis::result::AnalysisResult;

/// Criterion 5: `merge` unions `failed_files`. Two results that each name
/// the same file merge to a set of size 1, not 2.
#[test]
fn merge_unions_failed_files() {
    let mut a = AnalysisResult::default();
    a.failed_files.insert(PathBuf::from("src/lib.rs"));
    let mut b = AnalysisResult::default();
    b.failed_files.insert(PathBuf::from("src/lib.rs"));

    a.merge(b);

    assert_eq!(
        a.failed_files.len(),
        1,
        "the same file merged twice must collapse to one entry, got {:?}",
        a.failed_files
    );
    assert!(
        a.failed_files.contains(&PathBuf::from("src/lib.rs")),
        "the merged file must still be present, got {:?}",
        a.failed_files
    );
}

/// Criterion 6: `merge` concatenates findings and sums `dropped_out_of_range`.
#[test]
fn merge_concatenates_findings_and_sums_dropped_out_of_range() {
    let mut a = AnalysisResult::default();
    a.findings.push(Finding {
        kind: "k1".to_owned(),
        severity: Severity::Info,
        file_path: "f.py".to_owned(),
        line: 1,
        column: None,
        message: "m1".to_owned(),
        suggestion: None,
    });
    a.dropped_out_of_range = 2;

    let mut b = AnalysisResult::default();
    b.findings.push(Finding {
        kind: "k2".to_owned(),
        severity: Severity::Warning,
        file_path: "f.py".to_owned(),
        line: 2,
        column: None,
        message: "m2".to_owned(),
        suggestion: None,
    });
    b.dropped_out_of_range = 3;

    a.merge(b);

    assert_eq!(
        a.findings.len(),
        2,
        "findings must concatenate, got {:?}",
        a.findings
    );
    assert_eq!(
        a.findings[0].kind, "k1",
        "findings must concatenate in order, got {:?}",
        a.findings
    );
    assert_eq!(
        a.findings[1].kind, "k2",
        "findings must concatenate in order, got {:?}",
        a.findings
    );
    assert_eq!(
        a.dropped_out_of_range, 5,
        "dropped_out_of_range must sum, not take max or last"
    );
}

/// Criterion 7: `has_failures` is false on a default result and true once a
/// file is added.
#[test]
fn has_failures_reflects_failed_files() {
    let mut result = AnalysisResult::default();
    assert!(
        !result.has_failures(),
        "a default result has no failures, got true"
    );

    result.failed_files.insert(PathBuf::from("src/lib.rs"));
    assert!(
        result.has_failures(),
        "adding a file must flip has_failures to true"
    );
}

/// The BTreeSet field is what makes merging safe; `failed_files` is
/// declared as such so the merge test above is the contract. This pins
/// that the field is not accidentally a `Vec` or `HashSet` — a `Vec`
/// would silently break the union semantics and a `HashSet` would survive
/// but make test reproducibility harder.
#[test]
fn failed_files_is_a_btreeset() {
    let result = AnalysisResult::default();
    // The field is private-by-name; we test the public type via
    // construction. `BTreeSet<PathBuf>` is `Ord` and `Eq`, so
    // `default()` produces an empty set with the right element type.
    let _set: BTreeSet<PathBuf> = result.failed_files;
}
