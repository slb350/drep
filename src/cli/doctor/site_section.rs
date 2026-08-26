//! The `Site policy:` block of `drep doctor`.
//!
//! One line an operator cannot get anywhere else: whether a machine-level policy
//! is in effect here, and which file it came from. Without it, a repository that
//! behaves differently on one machine than on another has no visible cause.
//!
//! Nothing here gates. That matters most in the broken-policy arm: `drep check`
//! refuses to run on a policy file it cannot load, and this is the command
//! someone runs to find out why - so failing out would suppress the answer,
//! exactly as it would in the unreadable-auth-store arm of the LLM block.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use toml::Value;

use crate::config;
use crate::config::site::{SiteConfig, SiteConfigError};

/// `Site policy:` block, for all three states a policy file can be in.
pub(super) fn write_site_section<W: Write>(
    out: &mut W,
    path: &Path,
    loaded: &Result<Option<SiteConfig>, SiteConfigError>,
) -> Result<()> {
    writeln!(out)?;
    writeln!(out, "Site policy:")?;
    match loaded {
        // The path is named even though there is nothing there: an operator who
        // wants to install policy needs to know where it goes.
        Ok(None) => writeln!(out, "  none - no policy file at {}", path.display())?,
        Ok(Some(site)) => {
            writeln!(out, "  in effect from {}", path.display())?;
            match site.max_concurrent_ceiling {
                Some(ceiling) => writeln!(out, "  max_concurrent ceiling: {ceiling}")?,
                None => writeln!(out, "  no max_concurrent ceiling")?,
            }
        }
        // The error's own message names the file and states that `drep check`
        // refuses to run. Written once, in `SiteConfigError`, so the gate and the
        // diagnostic cannot describe the same failure differently.
        Err(err) => writeln!(out, "  {err}")?,
    }
    Ok(())
}

/// What the ceiling does to one raw `[[llm]]` entry, or `None` if nothing.
///
/// Printed only when the ceiling actually lowers this entry: a note on an entry
/// that was already below it reports a change that did not happen.
pub(super) fn clamp_note(entry: &Value, site: Option<&SiteConfig>) -> Option<String> {
    let requested = entry_max_concurrent(entry);
    let clamped = site?.clamp_concurrency(requested);
    (clamped < requested).then(|| {
        format!("max_concurrent: {requested} lowered to {clamped} by the site policy ceiling")
    })
}

/// The `max_concurrent` one raw entry will effectively run at.
///
/// The default comes from `LlmConfig::default()` rather than a literal, for the
/// reason `entry_is_enabled` does: this cannot then disagree with what
/// `config::load` decides about the same file. A value that does not fit a
/// `usize` also falls back to it - `config::load`'s own failure line already
/// names that problem precisely, and a second wording of it here would be a
/// worse one.
fn entry_max_concurrent(entry: &Value) -> usize {
    entry
        .get("max_concurrent")
        .and_then(Value::as_integer)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_else(|| config::LlmConfig::default().max_concurrent)
}
