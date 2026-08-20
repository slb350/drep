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
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::{Args, builder::TypedValueParser};

pub mod config_file;
pub mod gitignore;
pub mod hooks;
pub mod presets;
pub mod wizard;

use crate::Exit;
use crate::auth;
use crate::diff;

pub use hooks::HookKind;

#[cfg(test)]
mod tests;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Repository to install into.
    #[arg(long, value_name = "DIR", default_value = ".")]
    pub path: PathBuf,

    /// Which model provider to configure. Omit it for the interactive wizard.
    //
    // `Option` rather than a defaulted `String` so the command can tell "not
    // given" from "given as local", which is what decides whether the wizard
    // runs at all. A plain comment, not a doc comment: clap renders those as
    // help text, and the reason a field has its type is not something a user
    // asked about.
    #[arg(long, value_parser = provider_parser())]
    pub provider: Option<String>,

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

    /// Leave .gitignore alone.
    ///
    /// By default `drep init` adds `drep.toml` to it. The file holds no
    /// secrets, so this decides whether your provider choice is shared with
    /// the repository.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Never prompt, even on a terminal. For scripts and CI.
    #[arg(long, conflicts_with = "interactive")]
    pub non_interactive: bool,

    /// Always prompt, even when stdin is not a terminal. For a wrapper feeding
    /// answers on stdin.
    #[arg(long)]
    pub interactive: bool,
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
    run_with(&mut out, args, &auth::default_path()?).await
}

/// `run_to`, against a named auth store.
///
/// The store path is a parameter for the same reason `check::run_with` takes a
/// root: it is user-level state outside the repository, and a test that used the
/// real one would read the developer's own keys - making `key_in_store`, and so
/// the rendered `drep.toml`, depend on whose machine the suite ran on. It would
/// also write to it.
pub async fn run_with<W: Write>(out: &mut W, args: &InitArgs, auth_path: &Path) -> Result<Exit> {
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

    // Read before anything is written, so a broken store fails the command
    // rather than half-applying it.
    let store = auth::AuthStore::load(auth_path)?;

    let interactive = is_interactive(args);

    // Settled *before* a single question is asked, because refusing afterwards
    // half-applies the run: the wizard's own side effect is storing the pasted
    // key, and that happens before the config is written. Asking seven
    // questions, saving a credential and then failing on "drep.toml already
    // exists" leaves the store changed, the config not, and the provider not
    // switched.
    let force = {
        let mut console = wizard::Terminal::new(out);
        match existing_config(&root, args, interactive, &mut console)? {
            Some(force) => force,
            None => return Ok(Exit::Clean),
        }
    };

    let plan = if interactive {
        let mut console = wizard::Terminal::new(out);
        wizard::run(&mut console, &store, &crate::llm::models::Http).await?
    } else {
        plan_from_flags(args, &store)?
    };

    apply(out, &root, plan, store, auth_path, force).await?;

    Ok(Exit::Clean)
}

/// Decide what to do about a `drep.toml` that is already there.
///
/// Returns `Some(force)` to continue - `force` being whether the write may
/// replace the file - or `None` to stop having changed nothing.
///
/// The non-interactive answer is the one `init` has always given: refuse and
/// name `--force`. Scripts depend on that, and a script has nobody to ask.
/// Interactively the file is *shown* first, because "replace it?" is not a
/// question anyone can answer without knowing what is currently configured.
pub(crate) fn existing_config(
    root: &Path,
    args: &InitArgs,
    interactive: bool,
    console: &mut dyn wizard::Console,
) -> Result<Option<bool>> {
    let path = root.join(crate::config::default_config_path());
    if args.force || !path.exists() {
        return Ok(Some(args.force));
    }

    if !interactive {
        return Err(config_file::already_exists(&path));
    }

    console.say(&format!("{} already configures:", path.display()))?;
    for line in describe(&path) {
        console.say(&format!("  {line}"))?;
    }
    console.say("")?;

    if wizard::confirm(console, "Replace it?", false)? {
        return Ok(Some(true));
    }

    console.say("Left unchanged. `drep auth login` rotates a key without touching this file.")?;
    Ok(None)
}

/// One line per provider in an existing config, for the replace prompt.
///
/// Deliberately tolerant: this is describing a file to a user who is about to
/// overwrite it, so an unreadable or unparseable one must still let them say
/// yes rather than turning the prompt into an error about a file they are
/// discarding anyway.
pub(crate) fn describe(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return vec!["(could not be read)".to_string()];
    };
    // `toml::from_str::<Value>` and `raw.parse::<Value>()` are NOT
    // interchangeable in toml 1.x despite producing the same type: the former
    // runs the document parser, the latter runs `ValueDeserializer`, which
    // parses a single TOML *value* and rejects a whole document. Getting this
    // wrong reported every well-formed config as unparseable.
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return vec!["(could not be parsed)".to_string()];
    };
    let Some(entries) = value.get("llm").and_then(toml::Value::as_array) else {
        return vec!["(no [[llm]] provider)".to_string()];
    };
    if entries.is_empty() {
        return vec!["(no [[llm]] provider)".to_string()];
    }

    entries
        .iter()
        .map(|entry| {
            let field = |name: &str| {
                entry
                    .get(name)
                    .and_then(toml::Value::as_str)
                    .unwrap_or("(unset)")
                    .to_string()
            };
            let disabled = match entry.get("enabled").and_then(toml::Value::as_bool) {
                Some(false) => " (disabled)",
                _ => "",
            };
            format!("{} at {}{disabled}", field("model"), field("endpoint"))
        })
        .collect()
}

/// Whether to run the wizard.
///
/// Interactive when a person is at the other end and has not already said which
/// provider they want. `--provider` is the escape hatch that keeps every
/// existing scripted invocation working unchanged, and `--non-interactive`
/// covers the case of a script that wants the defaults without naming one.
///
/// The terminal check matters on its own: a hook, a CI job or a piped
/// invocation has no stdin to answer with, and prompting there would hang the
/// command rather than fail it.
fn is_interactive(args: &InitArgs) -> bool {
    use std::io::IsTerminal;
    wants_wizard(args, std::io::stdin().is_terminal())
}

/// The decision itself, with the terminal check as a parameter.
///
/// Split out because `std::io::stdin().is_terminal()` is *always false* under
/// `cargo test` - the harness captures stdin - so every combination of these
/// flags collapses to the same answer in-process and no unit test can tell the
/// conditions apart. The wiring to the real terminal is covered by an
/// integration test that runs the binary with a piped stdin; everything else is
/// covered here.
pub(crate) fn wants_wizard(args: &InitArgs, stdin_is_terminal: bool) -> bool {
    // Explicit beats inference, in both directions.
    if args.interactive {
        return true;
    }
    if args.non_interactive {
        return false;
    }
    // Naming a provider is answering the wizard's first question, so there is
    // nothing left to ask that a flag has not already said.
    args.provider.is_none() && stdin_is_terminal
}

/// Build the plan from flags alone, the way `init` always worked.
///
/// `--provider` defaults to `local` here rather than on the argument, because
/// the argument has to stay `None`-able for [`is_interactive`] to read.
pub(crate) fn plan_from_flags(args: &InitArgs, store: &auth::AuthStore) -> Result<wizard::Plan> {
    let provider = args.provider.as_deref().unwrap_or("local");
    let preset =
        presets::preset(provider).ok_or_else(|| anyhow!("unknown provider `{provider}`"))?;

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

    Ok(wizard::Plan {
        // A key already stored for this endpoint is used, and the `${VAR}` line
        // omitted - an explicit `api_key` would otherwise override the very key
        // the user saved, with a variable they may never have exported.
        choices: vec![config_file::Choice {
            preset,
            key_in_store: store.get(&endpoint).is_some(),
            model,
            endpoint,
        }],
        new_keys: Vec::new(),
        hooks: args.hooks,
        gitignore: !args.no_gitignore,
    })
}

/// Carry out a plan: store keys, write the config, install hooks, edit
/// `.gitignore`.
///
/// Keys first. A `drep.toml` naming no `api_key` is only correct once the store
/// holds one, so writing the config first would leave a window - and, if the
/// store write then failed, a config that authenticates as nothing with no
/// indication why.
async fn apply<W: Write>(
    out: &mut W,
    root: &Path,
    plan: wizard::Plan,
    mut store: auth::AuthStore,
    auth_path: &Path,
    force: bool,
) -> Result<()> {
    if !plan.new_keys.is_empty() {
        for (endpoint, key) in &plan.new_keys {
            store.set(endpoint, key)?;
        }
        store.save(auth_path)?;
        writeln!(out)?;
        writeln!(
            out,
            "✓ Stored {} key(s) for this machine (`drep auth list` to review)",
            plan.new_keys.len()
        )?;
    }

    let path = config_file::write(root, &config_file::render_chain(&plan.choices), force)?;

    let summary = plan
        .choices
        .iter()
        .map(|c| format!("{} ({})", c.preset.display_name, c.model))
        .collect::<Vec<_>>()
        .join(", then ");
    writeln!(out, "✓ Wrote {} - {summary}", path.display())?;

    if plan.gitignore {
        gitignore::ensure_to(out, root).await?;
    }

    hooks::install(out, root, plan.hooks, force).await?;

    // Every variable this config depends on, named whether or not it is set.
    // Naming it unconditionally is the point: the report is what tells a user
    // which variable this provider reads, and suppressing it once the variable
    // happens to be exported would hide that from the person most likely to
    // need it later. Whether it is *currently* set is the second column.
    //
    // Only providers whose key is not in the store appear: for those, the
    // rendered block carries no `api_key` line and the environment is never
    // consulted.
    let mut needed: Vec<&str> = Vec::new();
    for var in plan
        .choices
        .iter()
        .filter(|choice| !choice.key_in_store)
        .filter_map(|choice| choice.preset.api_key_env)
    {
        if !needed.contains(&var) {
            needed.push(var);
        }
    }

    if !needed.is_empty() {
        writeln!(out)?;
        writeln!(out, "This config reads its key from the environment:")?;
        for var in needed {
            match std::env::var_os(var) {
                Some(_) => writeln!(out, "  {var} - already set")?,
                None => writeln!(out, "  {var} - NOT set; export it before running drep")?,
            }
        }
    }

    Ok(())
}
