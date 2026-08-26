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
use crate::config::site::{Refusal, SiteConfig, SiteConfigError};

/// `Site policy:` block, for all three states a policy file can be in.
///
/// `refusal` is the marker probe's answer, evaluated by the caller through the
/// same `SiteConfig::refusal_for` the gate uses, and passed in rather than taken
/// here because this function is synchronous and the probe asks git. All three of
/// its states print: refused, not refused, and could not be evaluated.
pub(super) fn write_site_section<W: Write>(
    out: &mut W,
    path: &Path,
    loaded: &Result<Option<SiteConfig>, SiteConfigError>,
    refusal: &Result<Option<Refusal>, SiteConfigError>,
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
            write_markers(out, site, refusal)?;
        }
        // The error's own message names the file and states that `drep check`
        // refuses to run. Written once, in `SiteConfigError`, so the gate and the
        // diagnostic cannot describe the same failure differently.
        Err(err) => writeln!(out, "  {err}")?,
    }
    Ok(())
}

/// The configured markers, and what they do to *this* repository.
///
/// The list alone is not the answer an operator needs. "`refuse_markers` is set"
/// and "this checkout is refused" are different facts, and a report that only
/// carried the first would leave them guessing at the second - which is the
/// question they actually came with, because `drep check` has just exited 2.
///
/// The effect line is printed only when a marker is configured. Reporting "not
/// refused" on a machine with no marker policy would be a line about a mechanism
/// nobody here is using.
fn write_markers<W: Write>(
    out: &mut W,
    site: &SiteConfig,
    refusal: &Result<Option<Refusal>, SiteConfigError>,
) -> Result<()> {
    if site.refuse_markers.is_empty() {
        return writeln!(out, "  no refuse_markers").map_err(Into::into);
    }
    writeln!(out, "  refuse_markers: {}", site.refuse_markers.join(", "))?;
    match refusal {
        Ok(Some(refusal)) => writeln!(
            out,
            "  semantic review is refused here: {} is present",
            refusal.marker.display()
        )?,
        Ok(None) => writeln!(out, "  none of those files is here, so review runs")?,
        // Same reasoning as the broken-file arm above: the error's own message is
        // the single wording of this failure, and `drep check` fails closed on it.
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
