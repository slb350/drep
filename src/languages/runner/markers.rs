//! Whether a project has opted into a tool, by the configuration files its spec names.

use std::ffi::OsString;
use std::path::Path;

use crate::languages::spec::ToolSpec;

/// Whether the project has opted into this tool.
///
/// "Style adherence where defined": a repo with no eslint config has not
/// chosen eslint's defaults, so running it would invent findings the
/// project never asked for.
pub fn is_configured(spec: &ToolSpec, root: &Path) -> bool {
    configured_marker(spec, root).is_some()
}

/// Which of `config_files` is present in `root`, in declaration order.
///
/// One definition of "which config file is here", because two callers need
/// the answer and they need slightly different halves of it: eligibility
/// wants only whether there was one, while `config_flag` has to pass the
/// *name* to a tool that will not look for its own.
///
/// The list reads as a preference rather than as whatever the filesystem
/// returns, so the first declared match wins. When a spec declares multiple
/// `*.ext` globs (such as `*.sln` and `*.csproj`), `root` is read at most once.
pub(crate) fn configured_marker(spec: &ToolSpec, root: &Path) -> Option<String> {
    let mut entries: Option<Vec<OsString>> = None;
    for name in spec.config_files {
        let Some(extension) = name.strip_prefix("*.") else {
            if root.join(name).exists() {
                return Some((*name).to_owned());
            }
            continue;
        };
        let files = entries.get_or_insert_with(|| {
            let Ok(read_dir) = std::fs::read_dir(root) else {
                // An unreadable directory holds no marker we can see. `exists()`
                // answers false the same way for a path we cannot stat.
                return Vec::new();
            };
            read_dir.flatten().map(|entry| entry.file_name()).collect()
        });
        if let Some(found) = files.iter().find(|found| {
            // A *file* with the extension: a directory named `Widget.csproj` is
            // not a project, and counting it would run the tool one level above
            // the project it was meant to find. Stat only after the extension
            // matches so symlinks follow normally without stating every entry.
            Path::new(found)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
                && root.join(found).is_file()
        }) {
            return Some(found.to_string_lossy().into_owned());
        }
    }
    None
}

/// Whether `path` is what the marker `name` names in `workspace`: a `*.ext` marker any file directly in `workspace` with that extension, whatever its case, and any other marker the one path it spells there.
pub(crate) fn marker_names_path(workspace: &Path, name: &str, path: &Path) -> bool {
    match name.strip_prefix("*.") {
        Some(extension) => {
            path.parent() == Some(workspace)
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        }
        None => path == workspace.join(name),
    }
}
