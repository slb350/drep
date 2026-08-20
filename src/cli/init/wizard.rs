//! The interactive half of `drep init`.
//!
//! `drep init --provider kimi` is the scripted path and stays exactly as it
//! was. This is what runs when nobody passed `--provider` and there is a person
//! at the other end: pick a provider, paste a key, add a fallback, choose the
//! hooks, decide whether the config is committed.
//!
//! ## Everything goes through [`Console`]
//!
//! The wizard never touches stdin or stdout directly. That is what makes it
//! testable without a terminal - the tests drive it with a scripted queue of
//! answers and read back everything it said - and it is the same reason
//! `init::run_to` writes to a `&mut dyn Write` rather than to stdout.
//!
//! It also keeps the one genuinely awkward operation, reading a key without
//! echoing it, behind a single method. `rpassword` is used for that in
//! production and nothing in this file knows about it.
//!
//! ## The wizard decides, it does not act
//!
//! [`run`] returns a [`Plan`]. It writes no file, stores no key and installs no
//! hook. `init` applies the plan afterwards, in the same order and through the
//! same functions the flag path uses, so an answer given interactively cannot
//! reach a different code path than the equivalent flag.

use anyhow::{Result, anyhow};

use super::config_file::Choice;
use super::hooks::HookKind;
use super::presets::{self, LlmPreset};
use crate::auth::AuthStore;
use crate::llm::models::{Model, ModelSource};

/// The terminal, or a scripted stand-in.
pub trait Console {
    /// Print a line.
    fn say(&mut self, line: &str) -> Result<()>;

    /// Ask a question and read one line.
    ///
    /// `default` is shown in the prompt and returned when the answer is empty,
    /// so pressing Enter accepts it.
    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<String>;

    /// Ask for a secret and read one line without echoing it.
    ///
    /// An empty answer is legitimate and means "skip", so this cannot signal
    /// refusal by returning an error.
    fn ask_secret(&mut self, question: &str) -> Result<String>;
}

/// What the wizard decided, for `init` to carry out.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The failover chain, head first.
    pub choices: Vec<Choice>,
    /// `(endpoint, key)` pairs to write to the auth store.
    ///
    /// Separate from `choices` because storing a key is a side effect on the
    /// machine rather than on the repository, and the two are applied by
    /// different code with different failure modes.
    pub new_keys: Vec<(String, String)>,
    /// Which git hooks to install.
    pub hooks: HookKind,
    /// Whether to add `drep.toml` to `.gitignore`.
    pub gitignore: bool,
}

/// Run the wizard against `console`, consulting `store` for keys already held
/// and `source` for what each endpoint actually serves.
pub async fn run<S: ModelSource>(
    console: &mut dyn Console,
    store: &AuthStore,
    source: &S,
) -> Result<Plan> {
    console.say("Setting up drep. Enter accepts the value in brackets.")?;
    console.say("")?;

    let mut choices = Vec::new();
    let mut new_keys: Vec<(String, String)> = Vec::new();

    loop {
        let position = choices.len() + 1;
        let (choice, key) = one_provider(console, store, source, &new_keys, position).await?;
        if let Some(pair) = key {
            new_keys.push(pair);
        }
        choices.push(choice);

        console.say("")?;
        if !confirm(console, "Add a fallback provider?", false)? {
            break;
        }
        console.say("")?;
    }

    console.say("")?;
    let hooks = ask_hooks(console)?;
    console.say("")?;
    let gitignore = confirm(
        console,
        "Add drep.toml to .gitignore? (it holds no secrets, so committing it \
         shares your provider choice with the repo)",
        true,
    )?;

    Ok(Plan {
        choices,
        new_keys,
        hooks,
        gitignore,
    })
}

/// Ask for one provider: which, where, which model, and its key.
///
/// Returns the choice and, when the user pasted one, the key to store.
async fn one_provider<S: ModelSource>(
    console: &mut dyn Console,
    store: &AuthStore,
    source: &S,
    pending: &[(String, String)],
    position: usize,
) -> Result<(Choice, Option<(String, String)>)> {
    let preset = ask_provider(console, position)?;

    let endpoint = ask_required(console, "Endpoint", preset.endpoint)?;

    // The key is settled *before* the model, which is the whole reason the
    // endpoint can be asked what it serves: a listing needs authenticating.
    let key = ask_key(console, store, pending, preset, &endpoint)?;
    let model = ask_model(console, source, preset, &endpoint, key.usable.as_deref()).await?;
    let endpoint_for_store = endpoint.clone();

    Ok((
        Choice {
            preset,
            model,
            endpoint,
            key_in_store: key.in_store,
        },
        key.to_store.map(|stored| (endpoint_for_store, stored)),
    ))
}

/// Offer the models the endpoint actually serves, falling back to typing a name.
///
/// The fallback is not an edge case: a local llama.cpp build, a gateway, or any
/// endpoint older than its vendor's listing route will land there, and setup has
/// to continue exactly as it did before. Every failure is reported and stepped
/// past, never returned.
async fn ask_model<S: ModelSource>(
    console: &mut dyn Console,
    source: &S,
    preset: &LlmPreset,
    endpoint: &str,
    key: Option<&str>,
) -> Result<String> {
    match source
        .list(endpoint, key.unwrap_or(""), preset.protocol())
        .await
    {
        Ok(models) => choose_model(console, &models, preset.default_model),
        Err(err) => {
            console.say(&format!("  Could not list models: {err}"))?;
            ask_required(console, "Model", preset.default_model)
        }
    }
}

/// Pick from a listing, or type a name that is not in it.
///
/// A name outside the list is accepted rather than rejected: a model released
/// this morning is exactly the one somebody is trying to configure, and a menu
/// that refused it would be worse than the free-text prompt it replaced.
fn choose_model(
    console: &mut dyn Console,
    models: &[Model],
    preferred: Option<&str>,
) -> Result<String> {
    console.say("  This endpoint serves:")?;
    for (index, model) in models.iter().enumerate() {
        console.say(&format!("   {}. {}", index + 1, model.label()))?;
    }

    // The preset's default, if the endpoint still serves it. When it does not,
    // saying so is the signal that the shipped default has gone stale - which
    // is the failure this whole listing exists to remove.
    let default = preferred.and_then(|id| models.iter().position(|model| model.id == id));
    if default.is_none()
        && let Some(id) = preferred
    {
        console.say(&format!(
            "  (drep's usual default `{id}` is not in this list.)"
        ))?;
    }
    let default = default.map(|index| (index + 1).to_string());

    loop {
        let answer = console.ask("  Number or model name", default.as_deref())?;
        let answer = answer.trim();

        if answer.is_empty() {
            console.say("  Pick a number, or type a model name.")?;
            continue;
        }
        // Only a bare integer is a selection. Anything else is a name - which
        // is how a model too new to be listed still gets configured.
        match answer.parse::<usize>() {
            Ok(number) if (1..=models.len()).contains(&number) => {
                return Ok(models[number - 1].id.clone());
            }
            Ok(_) => console.say(&format!("  Enter a number from 1 to {}.", models.len()))?,
            Err(_) => return Ok(answer.to_string()),
        }
    }
}

/// Offer the preset table and read a selection.
fn ask_provider(console: &mut dyn Console, position: usize) -> Result<&'static LlmPreset> {
    let label = match position {
        1 => "Which provider?".to_string(),
        n => format!("Which provider for fallback #{}?", n - 1),
    };
    console.say(&label)?;

    let presets = presets::PRESETS;
    for (index, preset) in presets.iter().enumerate() {
        console.say(&format!(
            "  {}. {} - {}",
            index + 1,
            preset.display_name,
            preset.description
        ))?;
    }

    loop {
        let answer = console.ask("Number", Some("1"))?;
        match answer.trim().parse::<usize>() {
            Ok(n) if (1..=presets.len()).contains(&n) => return Ok(presets[n - 1]),
            _ => console.say(&format!("Enter a number from 1 to {}.", presets.len()))?,
        }
    }
}

/// Ask until a non-empty answer arrives, offering `default` if there is one.
///
/// A preset with no default (`custom`) has no value to fall back on, so an
/// empty answer has to be re-asked rather than accepted - writing an empty
/// endpoint would produce a config `config::load` rejects, reported as a
/// success by the command that wrote it.
fn ask_required(console: &mut dyn Console, label: &str, default: Option<&str>) -> Result<String> {
    loop {
        let answer = console.ask(label, default)?;
        let answer = answer.trim();
        if !answer.is_empty() {
            return Ok(answer.to_string());
        }
        console.say(&format!("{label} cannot be empty."))?;
    }
}

/// Resolve this provider's key: reuse a stored one, paste a new one, or fall
/// back to the environment variable.
///
/// Returns whether the rendered block should omit `api_key`, and the pair to
/// store when one was pasted.
fn ask_key(
    console: &mut dyn Console,
    store: &AuthStore,
    pending: &[(String, String)],
    preset: &LlmPreset,
    endpoint: &str,
) -> Result<KeyChoice> {
    // A preset that needs no key at all - a local server - has nothing to ask.
    // `usable` stays `None`, and the listing is attempted unauthenticated,
    // which is what such a server expects.
    let Some(env) = preset.api_key_env else {
        return Ok(KeyChoice::none());
    };

    // Already held, either on disk or pasted earlier in this same run for a
    // provider sharing the endpoint. Asking again would invite the user to
    // overwrite a working key with a typo.
    let held = store.get(endpoint).map(str::to_string).or_else(|| {
        pending
            .iter()
            .find(|(stored, _)| crate::auth::normalise(stored) == crate::auth::normalise(endpoint))
            .map(|(_, key)| key.clone())
    });
    if let Some(existing) = held {
        console.say("  A key is already stored for this endpoint; reusing it.")?;
        console.say("  (`drep auth login` replaces it, without touching drep.toml.)")?;
        return Ok(KeyChoice {
            in_store: true,
            to_store: None,
            usable: Some(existing),
        });
    }

    if let Some(url) = preset.key_url {
        console.say(&format!("  Get a key: {url}"))?;
    }

    // Reported because it changes what the empty answer means: with the
    // variable already exported, skipping is a complete setup rather than a
    // deferred one.
    if std::env::var_os(env).is_some() {
        console.say(&format!("  {env} is already set in this shell."))?;
    }

    let key = console.ask_secret(&format!(
        "  Paste your API key (or Enter to use ${{{env}}} instead)"
    ))?;

    if key.trim().is_empty() {
        console.say(&format!(
            "  No key stored. drep will read {env} from the environment."
        ))?;
        // The exported value, when there is one, still authenticates the model
        // listing - the user skipped storing a key, not using one.
        return Ok(KeyChoice {
            in_store: false,
            to_store: None,
            usable: std::env::var(env).ok(),
        });
    }

    console.say("  Key stored for this machine, not in drep.toml.")?;
    let key = key.trim().to_string();
    Ok(KeyChoice {
        in_store: true,
        to_store: Some(key.clone()),
        usable: Some(key),
    })
}

/// What asking about a key settled.
struct KeyChoice {
    /// Whether the rendered block should omit `api_key`.
    in_store: bool,
    /// The key to write to the auth store, when one was pasted. The endpoint it
    /// belongs to is the one the caller already passed in.
    to_store: Option<String>,
    /// A key that can authenticate the model listing, from wherever it came.
    usable: Option<String>,
}

impl KeyChoice {
    /// No key, and none needed.
    fn none() -> Self {
        Self {
            in_store: false,
            to_store: None,
            usable: None,
        }
    }
}

/// Ask which hooks to install.
fn ask_hooks(console: &mut dyn Console) -> Result<HookKind> {
    console.say("Which git hooks?")?;
    console.say("  1. pre-push - review what you are about to push (recommended)")?;
    console.say("  2. pre-commit - review every commit; slower, and costs per commit")?;
    console.say("  3. both")?;
    console.say("  4. none - write the config only")?;

    loop {
        // No empty arm: both `Console` implementations substitute the default
        // for an empty answer, which is what `ask_provider` relies on too.
        match console.ask("Number", Some("1"))?.trim() {
            "1" => return Ok(HookKind::PrePush),
            "2" => return Ok(HookKind::PreCommit),
            "3" => return Ok(HookKind::Both),
            "4" => return Ok(HookKind::None),
            _ => console.say("Enter a number from 1 to 4.")?,
        }
    }
}

/// Ask a yes/no question. `default_yes` decides what Enter means.
///
/// Public because `init` asks one of its own before the wizard starts: whether
/// to replace an existing config. Sharing it keeps the two prompts answering to
/// the same vocabulary.
pub fn confirm(console: &mut dyn Console, question: &str, default_yes: bool) -> Result<bool> {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    loop {
        let answer = console.ask(&format!("{question} [{hint}]"), None)?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default_yes),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => console.say("Enter y or n.")?,
        }
    }
}

/// The real terminal.
///
/// Reads lines from stdin and secrets through `rpassword`, which turns echo off
/// for the duration of the read. A pasted key would otherwise sit in the
/// terminal's scrollback, which is the one place drep takes care not to put a
/// credential anywhere else - `LlmConfig`, `LlmClient` and `AuthStore` all
/// hand-write `Debug` for the same reason.
pub struct Terminal<'a, W: std::io::Write> {
    out: &'a mut W,
}

impl<'a, W: std::io::Write> Terminal<'a, W> {
    /// Wrap `out` as the wizard's console.
    pub fn new(out: &'a mut W) -> Self {
        Self { out }
    }
}

impl<W: std::io::Write> Console for Terminal<'_, W> {
    fn say(&mut self, line: &str) -> Result<()> {
        writeln!(self.out, "{line}")?;
        Ok(())
    }

    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<String> {
        match default {
            Some(value) => write!(self.out, "{question} [{value}]: ")?,
            None => write!(self.out, "{question}: ")?,
        }
        self.out.flush()?;

        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line)?;
        // End of input mid-wizard. Returning the default would silently accept
        // choices nobody made, so it is an error naming what happened.
        if read == 0 {
            return Err(anyhow!("input ended while `drep init` was still asking"));
        }

        let answer = line.trim();
        Ok(match (answer.is_empty(), default) {
            (true, Some(value)) => value.to_string(),
            _ => answer.to_string(),
        })
    }

    fn ask_secret(&mut self, question: &str) -> Result<String> {
        use std::io::IsTerminal;

        write!(self.out, "{question}: ")?;
        self.out.flush()?;

        // `rpassword` turns echo off on the controlling terminal, which means
        // opening `/dev/tty` - and that fails outright when there is none,
        // rather than degrading. A piped stdin has nothing to echo in the first
        // place: the data is not being typed, so there is no echo to suppress
        // and a plain read is both correct and the only thing that works.
        let secret = if std::io::stdin().is_terminal() {
            rpassword::read_password()?
        } else {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line.trim_end_matches(['\n', '\r']).to_string()
        };

        // The typed newline was consumed by whichever branch ran, so the next
        // line would otherwise start on the same row as the prompt.
        writeln!(self.out)?;
        Ok(secret)
    }
}

#[cfg(test)]
pub(crate) mod tests;
