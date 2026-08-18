//! `drep init` - write `drep.toml` and install the git hooks.
//!
//! Two things, in order: point drep at a model, and wire it into the
//! repository's commit/push flow. This is the only part of drep that can
//! damage something, which is why every failure mode is spelled out in the
//! submodules rather than collapsed into a single "best effort" call.
//!
//! All output goes through a `&mut dyn std::io::Write` so the command is
//! testable without spawning a subprocess.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, builder::TypedValueParser};

pub mod config_file;
pub mod hooks;
pub mod presets;

use crate::Exit;
use crate::diff;

pub use hooks::HookKind;

#[cfg(test)]
mod tests;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Repository to install into.
    #[arg(long, value_name = "DIR", default_value = ".")]
    pub path: PathBuf,

    /// Which model provider to configure.
    #[arg(long, default_value = "local", value_parser = provider_parser())]
    pub provider: String,

    /// Model name. Defaults to the preset's.
    #[arg(long)]
    pub model: Option<String>,

    /// Base URL. Required for `--provider custom`.
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Which git hooks to install.
    #[arg(long, value_enum, default_value_t = HookKind::PrePush)]
    pub hooks: HookKind,

    /// Overwrite an existing drep.toml or a drep-managed hook.
    #[arg(long)]
    pub force: bool,
}

/// Build the `--provider` value parser from [`presets::preset_keys`].
///
/// Same pattern `severity_parser` uses for `--fail-on`: the accepted set
/// comes from the preset table, so `--help` and clap's validator cannot
/// drift apart from the data that drives them.
fn provider_parser() -> impl TypedValueParser<Value = String> {
    use clap::builder::PossibleValuesParser;
    PossibleValuesParser::new(presets::preset_keys())
}

/// Run the command, writing to stdout. Returns `Ok(Exit::Clean)` on success
/// and `Err(_)` on any failure.
pub async fn run(args: &InitArgs) -> Result<Exit> {
    let mut out = std::io::stdout().lock();
    run_to(&mut out, args).await
}

/// `run`, writing to an arbitrary sink so tests can capture the report.
pub async fn run_to<W: Write>(out: &mut W, args: &InitArgs) -> Result<Exit> {
    let toplevel = match diff::run_git(&args.path, &["rev-parse", "--show-toplevel"]).await {
        Ok(s) => s,
        Err(_) => {
            return Err(anyhow!(
                "{} is not inside a git repository",
                args.path.display()
            ));
        }
    };
    let root = PathBuf::from(toplevel);

    let preset = presets::preset(&args.provider)
        .ok_or_else(|| anyhow!("unknown provider `{}`", args.provider))?;

    let endpoint = args
        .endpoint
        .clone()
        .or_else(|| preset.endpoint.map(str::to_owned))
        .ok_or_else(|| {
            anyhow!(
                "--provider {} needs an --endpoint (it presumes no host)",
                preset.key
            )
        })?;

    let model = args
        .model
        .clone()
        .or_else(|| preset.default_model.map(str::to_owned))
        .ok_or_else(|| anyhow!("--provider {} needs a --model", preset.key))?;

    let path = config_file::write(
        &root,
        &config_file::render(preset, &model, &endpoint),
        args.force,
    )?;

    writeln!(
        out,
        "✓ Wrote {} ({}, {})",
        path.display(),
        preset.display_name,
        model
    )?;

    hooks::install(out, &root, args.hooks, args.force).await?;

    if let Some(var) = preset.api_key_env {
        writeln!(out)?;
        writeln!(out, "Set your key before running drep:")?;
        writeln!(out, "  export {var}='...'")?;
    }

    Ok(Exit::Clean)
}
