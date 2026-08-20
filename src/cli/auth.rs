//! `drep auth` - manage the keys drep holds for this machine.
//!
//! The store is written by `drep init` as a side effect of setting a provider
//! up. These subcommands exist for everything after that: rotating a key,
//! adding one for an endpoint you configured by hand, checking what is held,
//! and removing one.
//!
//! **No subcommand ever prints a key.** `list` prints endpoints, `login` reads
//! one without echoing it, and `logout` reports only whether anything was
//! removed. A `drep auth show` would be the obvious convenience and is
//! deliberately absent: the store is a file, and anyone who genuinely needs the
//! value can read it, having chosen to.

use std::io::Write;
use std::path::Path;

use anyhow::{Result, anyhow};
use clap::{Args, Subcommand};

use crate::Exit;
use crate::auth::{AuthStore, default_path};
use crate::cli::init::presets;
use crate::cli::init::wizard::{Console, Terminal};

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// List the endpoints with a stored key. Never prints the keys.
    List,
    /// Store a key for an endpoint, reading it without echoing.
    Login(LoginArgs),
    /// Forget the key stored for an endpoint.
    Logout(LogoutArgs),
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// The endpoint the key authenticates.
    ///
    /// Mutually exclusive with `--provider`, which supplies one from the preset
    /// table.
    #[arg(long, conflicts_with = "provider")]
    pub endpoint: Option<String>,

    /// A preset whose endpoint to use, e.g. `kimi`.
    #[arg(long)]
    pub provider: Option<String>,
}

#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// The endpoint to forget.
    #[arg(long)]
    pub endpoint: String,
}

/// Run the command, writing to stdout.
pub fn run(args: &AuthArgs) -> Result<Exit> {
    let mut out = std::io::stdout().lock();
    run_at(&mut out, args, &default_path()?)
}

/// `run_to`, against a named store.
///
/// The path is a parameter for the same reason `init::run_with` takes one: the
/// store is user-level state, and a test using the real one would read and
/// write the developer's own keys.
pub fn run_at<W: Write>(out: &mut W, args: &AuthArgs, path: &Path) -> Result<Exit> {
    match &args.command {
        AuthCommand::List => list(out, path),
        AuthCommand::Login(login) => {
            let mut console = Terminal::new(out);
            self::login(&mut console, login, path)
        }
        AuthCommand::Logout(logout) => self::logout(out, logout, path),
    }
}

/// Print the endpoints with a stored key.
fn list<W: Write>(out: &mut W, path: &Path) -> Result<Exit> {
    let store = AuthStore::load(path)?;

    if store.is_empty() {
        writeln!(out, "No keys stored ({}).", path.display())?;
        writeln!(out, "Run `drep auth login --provider <name>` to add one.")?;
        return Ok(Exit::Clean);
    }

    writeln!(out, "Keys stored in {}:", path.display())?;
    for endpoint in store.endpoints() {
        // The preset name, when one matches, is what a user actually recognises;
        // the endpoint alone reads as a URL they half remember configuring.
        match matching_preset(endpoint).map(|p| p.display_name) {
            Some(name) => writeln!(out, "  {endpoint}  ({name})")?,
            None => writeln!(out, "  {endpoint}")?,
        }
    }
    Ok(Exit::Clean)
}

/// Read a key and store it for the resolved endpoint.
fn login(console: &mut dyn Console, args: &LoginArgs, path: &Path) -> Result<Exit> {
    let endpoint = resolve_endpoint(args)?;
    let mut store = AuthStore::load(path)?;

    if store.get(&endpoint).is_some() {
        console.say(&format!("Replacing the key stored for {endpoint}."))?;
    }

    if let Some(url) = matching_preset(&endpoint).and_then(|p| p.key_url) {
        console.say(&format!("Get a key: {url}"))?;
    }

    let key = console.ask_secret(&format!("Paste the key for {endpoint}"))?;
    // An empty paste is a cancellation, not a key. `AuthStore::set` would refuse
    // it anyway; saying so here is the difference between "you changed your
    // mind" and "something went wrong".
    if key.trim().is_empty() {
        console.say("No key entered; nothing was stored.")?;
        return Ok(Exit::Clean);
    }

    store.set(&endpoint, &key)?;
    store.save(path)?;
    console.say(&format!("✓ Stored a key for {endpoint}"))?;
    Ok(Exit::Clean)
}

/// Forget the key for an endpoint.
fn logout<W: Write>(out: &mut W, args: &LogoutArgs, path: &Path) -> Result<Exit> {
    let mut store = AuthStore::load(path)?;

    if !store.remove(&args.endpoint) {
        writeln!(out, "No key was stored for {}.", args.endpoint)?;
        return Ok(Exit::Clean);
    }

    store.save(path)?;
    writeln!(out, "✓ Forgot the key for {}", args.endpoint)?;
    Ok(Exit::Clean)
}

/// The endpoint `login` should use.
///
/// `--provider` is resolved through the preset table so the two ways of naming
/// an endpoint cannot disagree - a user who ran `drep init --provider kimi` and
/// then `drep auth login --provider kimi` must land on the same key.
fn resolve_endpoint(args: &LoginArgs) -> Result<String> {
    if let Some(endpoint) = &args.endpoint {
        return Ok(endpoint.clone());
    }

    let Some(name) = &args.provider else {
        return Err(anyhow!(
            "name the endpoint with --endpoint, or a preset with --provider"
        ));
    };

    let preset = presets::preset(name).ok_or_else(|| anyhow!("unknown provider `{name}`"))?;
    preset
        .endpoint
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("--provider {name} presumes no host; use --endpoint instead"))
}

/// The preset whose endpoint matches, compared the way the store compares.
fn matching_preset(endpoint: &str) -> Option<&'static presets::LlmPreset> {
    let wanted = crate::auth::normalise(endpoint);
    presets::PRESETS.iter().copied().find(|p| {
        p.endpoint
            .is_some_and(|e| crate::auth::normalise(e) == wanted)
    })
}

#[cfg(test)]
mod tests;
