//! Adding `drep.toml` to the repository's `.gitignore`.
//!
//! Whether the config belongs in version control is a judgement, not a fact.
//! It names an endpoint, a model and a protocol - shareable, and worth sharing
//! when a team wants one gate reviewing with one model - but it is also a
//! personal choice of provider that a collaborator may not have a plan for.
//! Since 2.1 the *key* is not in the file at all (it lives in the user-level
//! auth store), so this is a preference rather than a safety requirement, and
//! `drep init` asks rather than deciding.
//!
//! ## Why this is not one `writeln!`
//!
//! Two states make a naive append useless, and both are silent:
//!
//! - **Already ignored.** By this exact path, by a glob, or by a parent
//!   directory's rule. Appending again is a duplicate line that never
//!   changes behaviour.
//! - **Already tracked.** `.gitignore` has *no effect* on a file git already
//!   tracks. Appending there looks like it worked, `git status` keeps showing
//!   the file, and nothing explains why. The fix is `git rm --cached`, so that
//!   is what gets reported.
//!
//! Both are answered by asking git rather than by parsing `.gitignore`, which
//! is the only way to get glob and parent-directory rules right.

use std::io::Write;
use std::path::Path;

use anyhow::{Result, anyhow};

use crate::diff;

/// The entry written, and the comment that explains it.
///
/// A bare line in someone's `.gitignore` with no attribution is a small
/// mystery six months later; the comment names what put it there.
const ENTRY: &str = "drep.toml";
const COMMENT: &str = "# drep's local config: provider and model choice, no secrets.";

/// What `ensure` did, so the caller can report it precisely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The entry was appended to `.gitignore`.
    Added,
    /// The file was created and the entry written to it.
    Created,
    /// Git already ignores the path, by whatever rule. Nothing was written.
    AlreadyIgnored,
    /// Git tracks the file, so `.gitignore` cannot affect it. Nothing was
    /// written, because writing would have implied otherwise.
    Tracked,
}

impl Outcome {
    /// The line `init` prints.
    pub fn message(&self) -> String {
        match self {
            Self::Added => format!("✓ Added {ENTRY} to .gitignore"),
            Self::Created => format!("✓ Created .gitignore with {ENTRY}"),
            Self::AlreadyIgnored => format!("· {ENTRY} is already ignored"),
            Self::Tracked => format!(
                "! {ENTRY} is tracked by git, so .gitignore will not affect it.\n  \
                 Run `git rm --cached {ENTRY}` to stop tracking it, keeping the file."
            ),
        }
    }
}

/// Ensure `root`'s `.gitignore` ignores `drep.toml`, and report what happened.
///
/// Writes nothing when git already ignores or already tracks the path.
pub async fn ensure(root: &Path) -> Result<Outcome> {
    if is_tracked(root).await? {
        return Ok(Outcome::Tracked);
    }
    if is_ignored(root).await? {
        return Ok(Outcome::AlreadyIgnored);
    }
    append(root)
}

/// Ensure, then report to `out`. The shape `init`'s other steps use.
pub async fn ensure_to<W: Write>(out: &mut W, root: &Path) -> Result<Outcome> {
    let outcome = ensure(root).await?;
    writeln!(out, "{}", outcome.message())?;
    Ok(outcome)
}

/// Whether git answers yes to a question about `drep.toml`.
///
/// `git check-ignore` and `git ls-files --error-unmatch` both answer by exit
/// code, which `diff::git_query` turns into an `Option`. Asking git rather than
/// reading `.gitignore` is what makes a glob (`*.toml`) or a parent directory's
/// rule count, which no line-by-line comparison would catch.
async fn git_says_yes(root: &Path, args: &[&str], question: &str) -> Result<bool> {
    diff::git_query(root, args)
        .await
        .map(|answer| answer.is_some())
        .map_err(|err| anyhow!("could not ask git whether {ENTRY} is {question}: {err}"))
}

/// Whether git already ignores `drep.toml`.
async fn is_ignored(root: &Path) -> Result<bool> {
    git_says_yes(root, &["check-ignore", "--quiet", "--", ENTRY], "ignored").await
}

/// Whether git already tracks `drep.toml`.
///
/// The case where appending to `.gitignore` silently does nothing at all.
async fn is_tracked(root: &Path) -> Result<bool> {
    git_says_yes(
        root,
        &["ls-files", "--error-unmatch", "--", ENTRY],
        "tracked",
    )
    .await
}

/// Append the entry, creating `.gitignore` if it does not exist.
///
/// A missing trailing newline on the existing file is repaired before
/// appending, because otherwise the entry joins the last line and both rules
/// stop working - and the file that gets damaged is one drep did not write.
fn append(root: &Path) -> Result<Outcome> {
    let path = root.join(".gitignore");
    let existing = match std::fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(anyhow!("could not read {}: {err}", path.display())),
    };

    // Only whether the file existed survives; keeping the string alive to ask
    // that later would copy the whole file for a boolean.
    let created = existing.is_none();
    let mut body = existing.unwrap_or_default();
    if !body.is_empty() {
        // Terminate the last line if the file did not. Otherwise the comment
        // runs into it and *both* rules stop working - in a file drep did not
        // write.
        if !body.ends_with('\n') {
            body.push('\n');
        }
        // One blank line between what was there and what drep adds. Skipped for
        // an empty file, where it would be a leading blank line instead.
        body.push('\n');
    }
    body.push_str(COMMENT);
    body.push('\n');
    body.push_str(ENTRY);
    body.push('\n');

    std::fs::write(&path, body)
        .map_err(|err| anyhow!("could not write {}: {err}", path.display()))?;

    Ok(if created {
        Outcome::Created
    } else {
        Outcome::Added
    })
}
