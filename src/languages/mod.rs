//! Language registry: resolve a `Path` to the language that owns its extension.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

pub mod definitions;
pub mod runner;
pub mod spec;

// One public path per type, matching `analysis`: no facade re-exports, so
// consumers cannot drift between `languages::ToolSpec` and
// `languages::spec::ToolSpec`.
use spec::LanguageSupport;

/// The language owning `path`, or `None` if drep does not analyze it.
///
/// Case-insensitive, matching the rest of drep's file-target policy.
pub fn detect(path: &Path) -> Option<&'static LanguageSupport> {
    detect_index(path).map(|index| all_languages()[index])
}

/// The index of the language owning `path` within [`all_languages`].
///
/// The index *is* the language's identity, which is what [`group_by_language`]
/// needs; recovering it afterwards by comparing pointers meant scanning the
/// table twice for an answer the first scan already had.
fn detect_index(path: &Path) -> Option<usize> {
    // Extension first, whole name second, and never the other way round: a
    // name match must not shadow a language that claims the suffix, or a file
    // called `Dockerfile.ts` would stop being TypeScript.
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && let Some(index) = by_extension(ext)
    {
        return Some(index);
    }
    let name = path.file_name()?.to_str()?;
    by_filename(name).or_else(|| by_filename_stem(name))
}

/// The language claiming `ext`, which carries no leading dot.
///
/// No allocation: the table's extensions are ASCII literals that all begin
/// with a dot, so `[1..]` is the bare suffix and `eq_ignore_ascii_case` does
/// the case folding that `to_lowercase()` used to do into two throwaway
/// `String`s per call - on a function called once per walked file.
fn by_extension(ext: &str) -> Option<usize> {
    all_languages().iter().position(|lang| {
        lang.extensions
            .iter()
            .any(|known| known[1..].eq_ignore_ascii_case(ext))
    })
}

/// The language claiming the whole file name `name`.
///
/// `Path::extension` answers `None` for `Dockerfile`, `Makefile`, `Gemfile`
/// and `Jenkinsfile`, so without this they are dropped at language grouping
/// and reported as a clean run - the silent pass drep exists to refuse.
/// Case-insensitive, matching the extension lookup and the rest of drep's
/// file-target policy.
fn by_filename(name: &str) -> Option<usize> {
    all_languages().iter().position(|lang| {
        lang.filenames
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
    })
}

/// The language claiming `name` as a dotted variant of a claimed stem, e.g.
/// `Dockerfile.dev`.
///
/// Exact whole-name claims cover the canonical file; this covers the
/// unbounded family of per-environment variants multi-image layouts produce.
/// Runs after the exact lookup, and the extension lookup runs before both,
/// so `Dockerfile.ts` stays TypeScript. The variant must carry a non-empty
/// suffix after the dot: `Dockerfile.` itself is claimed by neither rule.
fn by_filename_stem(name: &str) -> Option<usize> {
    // Split once, rather than re-slicing `name` per registered stem. Every
    // stem is dot-free ASCII, asserted by
    // `every_registered_entry_is_well_formed`, so the text before the first
    // dot is the only thing any of them can match.
    let (head, variant) = name.split_once('.')?;
    if variant.is_empty() {
        return None;
    }
    all_languages().iter().position(|lang| {
        lang.filename_prefixes
            .iter()
            .any(|stem| stem.eq_ignore_ascii_case(head))
    })
}

/// Bucket `paths` by the language that owns them.
///
/// The single answer to "which languages are present here, and with which
/// files". Both `drep check`'s deterministic layer (which needs the batch per
/// tool) and `drep doctor` (which needs the counts) ask it, so neither
/// re-derives language identity from a path — and neither can disagree with
/// the other about what this repository contains, which is the specific thing
/// `doctor` exists to report truthfully.
///
/// Paths that no registered language claims are dropped: drep has no opinion
/// on a file type it does not analyze. The result is ordered by
/// registration order rather than by name, so output is
/// stable across runs and reads in the order the language table is written.
///
/// Borrows rather than owns. The caller already holds the paths, and every
/// consumer converts them again for its own use - to argv strings in the tool
/// runner, to counts in `doctor` - so cloning into the buckets was a copy that
/// nothing read.
pub fn group_by_language<'a>(paths: &[&'a Path]) -> Vec<(&'static LanguageSupport, Vec<&'a Path>)> {
    let mut buckets: Vec<Vec<&'a Path>> = vec![Vec::new(); all_languages().len()];
    for path in paths {
        if let Some(index) = detect_index(path) {
            buckets[index].push(path);
        }
    }
    buckets
        .into_iter()
        .enumerate()
        .filter(|(_, files)| !files.is_empty())
        .map(|(index, files)| (all_languages()[index], files))
        .collect()
}

/// Every registered language, in registration order.
pub fn all_languages() -> &'static [&'static LanguageSupport] {
    &definitions::ALL_LANGUAGES[..]
}

/// Every extension any registered language claims, lowercased, with the dot.
///
/// Derived from the registry so adding a language automatically widens the
/// scan target set. Duplicates collapse: JavaScript and TypeScript both own
/// `.ts`-adjacent extensions, so a hand-written list here would silently need
/// to track them.
pub fn source_extensions() -> &'static [&'static str] {
    &SOURCE_EXTENSIONS
}

/// Computed once so repeated registry introspection does not rebuild a set
/// whose answer is fixed at compile time.
static SOURCE_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for lang in all_languages() {
        for ext in lang.extensions {
            if seen.insert(*ext) {
                out.push(*ext);
            }
        }
    }
    out
});

/// Every dependency/build directory any registered language creates.
///
/// Same deduplication discipline as `source_extensions`: each `LanguageSupport`
/// declares its own vendored directories, and the set is built once.
pub fn vendored_dirs() -> &'static [&'static str] {
    &VENDORED_DIRS
}

/// Computed once, for the same reason as [`SOURCE_EXTENSIONS`].
static VENDORED_DIRS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for lang in all_languages() {
        for dir in lang.vendored_dirs {
            if seen.insert(*dir) {
                out.push(*dir);
            }
        }
    }
    out
});

#[cfg(test)]
mod tests;
