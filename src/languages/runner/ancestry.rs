//! Which directories a file's tool configuration is looked for in: the file's own directory and each one above it, up to the root drep runs from.

use std::path::{Component, Path, PathBuf};

pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// `start` and each directory above it, up to and including `root`, or none when `start` is not under `root`.
///
/// Both are made absolute and lexically normal first. A staged or branch file above the directory drep runs in arrives as `../top.js`, and `root/sub/..` compares as under `root/sub` component by component, which found `sub/`'s configuration for a file outside it.
pub(super) fn ancestors_within(start: &Path, root: &Path) -> Vec<PathBuf> {
    let root = lexically_normal(&absolute(root));
    let start = lexically_normal(&absolute(start));
    if !start.starts_with(&root) {
        return Vec::new();
    }
    start
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&root))
        .map(Path::to_path_buf)
        .collect()
}

/// `path` with each `.` dropped and each `..` taking back the component before it, without consulting the filesystem.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}
