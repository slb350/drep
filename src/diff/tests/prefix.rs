//! `from_prefix`: a path named from the top level of the working tree, as seen from the directory drep runs in.

use std::path::{Path, PathBuf};

use crate::diff::prefix::from_prefix;

fn seen(path: &str, prefix: &str) -> PathBuf {
    from_prefix(Path::new(path), Path::new(prefix))
}

#[test]
fn at_the_top_level_a_path_is_unchanged() {
    assert_eq!(seen("src/a.rs", ""), PathBuf::from("src/a.rs"));
}

#[test]
fn inside_the_working_directory_a_path_loses_its_prefix() {
    assert_eq!(seen("sub/doc.md", "sub/"), PathBuf::from("doc.md"));
    assert_eq!(seen("sub/deep/x.md", "sub/"), PathBuf::from("deep/x.md"));
}

#[test]
fn outside_the_working_directory_a_path_climbs_to_the_common_directory() {
    assert_eq!(seen("README.md", "sub/"), PathBuf::from("../README.md"));
    assert_eq!(
        seen("other/x.rs", "sub/deep/"),
        PathBuf::from("../../other/x.rs")
    );
    assert_eq!(seen("sub/y.rs", "sub/deep/"), PathBuf::from("../y.rs"));
}

#[test]
fn a_directory_sharing_only_the_start_of_its_name_is_another_directory() {
    assert_eq!(seen("subway/a.rs", "sub/"), PathBuf::from("../subway/a.rs"));
}
