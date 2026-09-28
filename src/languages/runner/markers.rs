//! Whether a project has opted into a tool, by the configuration files its spec names.

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
/// *name* to a tool that will not look for its own. The flag branch used to
/// ask separately with a bare `join(name).exists()`, which does not
/// understand the leading `*.` glob `marker_match` accepts - so a spec
/// pairing a glob marker with a flag would be judged configured and then run
/// without the flag it needs. Only the pairing of tools kept that dormant:
/// `dotnet format` already has glob markers and checkstyle already has a
/// flag.
///
/// The list reads as a preference rather than as whatever the filesystem
/// returns, so the first declared match wins.
pub(super) fn configured_marker(spec: &ToolSpec, root: &Path) -> Option<String> {
    spec.config_files
        .iter()
        .find_map(|name| marker_match(root, name))
}

/// The marker `name` claims in `root`, as the name of the file found.
///
/// A `*.ext` entry matches any file in the directory carrying that extension.
/// C# is the first language whose workspace is identified by a glob rather
/// than a fixed name: MSBuild has to run from the directory holding the
/// `.csproj` or `.sln`, and those are named after the project. Keying
/// `dotnet format` on `.editorconfig` instead - the file it reads its rules
/// from - picks whichever ancestor happens to hold one, which in a solution
/// laid out with projects in subdirectories is a directory with no project in
/// it at all, where MSBuild exits with "Could not find a project or solution
/// file" and drep reports a configured tool that could not run.
///
/// Only a leading `*.` is a glob. A name is otherwise taken literally, so
/// `.eslintrc.json` and the rest keep costing one `exists` rather than a
/// directory read.
fn marker_match(root: &Path, name: &str) -> Option<String> {
    if !name.starts_with("*.") {
        return root.join(name).exists().then(|| name.to_owned());
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory holds no marker we can see. `exists()`
        // answers false the same way for a path we cannot stat.
        return None;
    };
    entries.flatten().find_map(|entry| {
        let found = entry.file_name();
        // The extension test first: it is a string comparison that rejects
        // nearly every entry, while `file_type` falls back to an `lstat`
        // whenever `readdir` reports `DT_UNKNOWN` (network mounts, some FUSE
        // filesystems). This predicate runs once per ancestor directory per
        // file per spec, so the selective half belongs in front.
        //
        // A *file* with the extension: a directory named `Widget.csproj` is
        // not a project, and counting it would run the tool one level above
        // the project it was meant to find.
        let matches = marker_names_path(root, name, &root.join(&found))
            && entry.file_type().is_ok_and(|kind| kind.is_file());
        matches.then(|| found.to_string_lossy().into_owned())
    })
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
