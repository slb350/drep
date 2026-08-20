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
use crate::llm::quirks::{self, QuirksSource, Registry};

/// Whether an environment variable is set, in the real process.
///
/// The production answer for `run`'s `env_is_set`. Injected rather than read
/// inline so the wizard's tests need no `std::env::set_var`, which is `unsafe`
/// in edition 2024 because a concurrent reader on another thread is a data race
/// - and `cargo test` is multi-threaded.
pub fn real_env(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// What the wizard decided, for `init` to carry out.
#[derive(Clone)]
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

/// Run the wizard against `console`, consulting `store` for keys already held,
/// `source` for what each endpoint actually serves and `quirks_source` for what
/// the chosen model accepts.
pub async fn run<S: ModelSource, Q: QuirksSource>(
    console: &mut dyn Console,
    deps: Deps<'_, S, Q>,
) -> Result<Plan> {
    console.say("Setting up drep. Enter accepts the value in brackets.")?;

    // Fetched on first use, not here. Every provider needs it and it is one
    // document rather than one per endpoint, so it is fetched at most once - but
    // doing it before the first prompt meant an offline `drep init` sat through
    // the whole timeout and then opened with an error about a service the user
    // may never reach. Deferring it puts any wait after the questions the user
    // came to answer, and a warm cache makes it invisible either way.
    let mut registry = LazyRegistry::new();
    let mut codex_status: Option<Result<crate::llm::codex::CodexStatus, String>> = None;
    console.say("")?;

    let mut choices = Vec::new();
    let mut new_keys: Vec<(String, String)> = Vec::new();

    loop {
        let position = choices.len() + 1;
        let (choice, key) = one_provider(
            console,
            &deps,
            &mut registry,
            &mut codex_status,
            &new_keys,
            position,
        )
        .await?;
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

/// Hand-written so a pasted key cannot reach a log.
///
/// `new_keys` holds credentials, so a derived `Debug` would print every one of
/// them from any `{:?}`, `dbg!`, or `expect` message that touched a `Plan`. The
/// same reason `LlmConfig`, `LlmClient` and `AuthStore` all write theirs.
impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("choices", &self.choices)
            .field(
                "new_keys",
                &self
                    .new_keys
                    .iter()
                    .map(|(endpoint, _)| endpoint.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("hooks", &self.hooks)
            .field("gitignore", &self.gitignore)
            .finish()
    }
}

/// What the wizard needs from the outside world.
///
/// Bundled rather than threaded one by one: every field here exists so a test
/// can supply a stand-in - the model listing, the quirks registry, and whether
/// an environment variable is set are the three things that would otherwise
/// reach the network or the process environment from inside a unit test.
///
/// The bounds sit on the functions that use the fields rather than on the
/// struct: a bound on a data declaration has to be repeated at every mention
/// of the type without constraining anything the fields do.
pub struct Deps<'a, S, Q> {
    /// Keys already held for this machine.
    pub store: &'a AuthStore,
    /// What models an endpoint serves.
    pub source: &'a S,
    /// What quirks a chosen model has.
    pub quirks_source: &'a Q,
    /// Whether an environment variable is set.
    pub env_is_set: &'a dyn Fn(&str) -> bool,
    /// Whether the Codex CLI has usable ChatGPT-managed authentication.
    pub(crate) codex_status: &'a dyn Fn() -> Result<crate::llm::codex::CodexStatus, String>,
}

/// The registry, fetched at most once and only when something needs it.
///
/// `None` after a failed attempt is remembered, so a provider chain does not
/// retry a fetch that already failed once per entry - and does not report the
/// same failure repeatedly.
struct LazyRegistry {
    fetched: bool,
    registry: Option<Registry>,
}

impl LazyRegistry {
    fn new() -> Self {
        Self {
            fetched: false,
            registry: None,
        }
    }

    /// The registry, fetching it the first time and reporting a failure once.
    async fn get<Q: QuirksSource>(
        &mut self,
        console: &mut dyn Console,
        source: &Q,
    ) -> Option<&Registry> {
        if !self.fetched {
            self.fetched = true;
            match source.registry().await {
                Ok(registry) => self.registry = Some(registry),
                Err(err) => {
                    // Said, never returned: nothing about model quirks may stop
                    // `drep init`.
                    let _ = console.say(&format!("  Could not check model quirks: {err}"));
                    let _ = console.say("  Falling back to this provider's own defaults.");
                }
            }
        }
        self.registry.as_ref()
    }
}

/// Ask for one provider: which, where, which model, and its key.
///
/// Returns the choice and, when the user pasted one, the key to store.
async fn one_provider<S: ModelSource, Q: QuirksSource>(
    console: &mut dyn Console,
    deps: &Deps<'_, S, Q>,
    registry: &mut LazyRegistry,
    codex_status: &mut Option<Result<crate::llm::codex::CodexStatus, String>>,
    pending: &[(String, String)],
    position: usize,
) -> Result<(Choice, Option<(String, String)>)> {
    let preset = ask_provider(console, position)?;

    if matches!(preset.backend, presets::PresetBackend::Codex(_)) {
        let status = codex_status
            .get_or_insert_with(|| (deps.codex_status)())
            .as_ref()
            .map_err(|err| anyhow!(err.clone()))?;
        console.say(&format!(
            "  Codex CLI {} is authenticated through ChatGPT.",
            status.cli_version()
        ))?;
        let model = ask_required(console, "Model", preset.default_model)?;
        return Ok((Choice::codex(preset, model), None));
    }

    let endpoint = ask_required(console, "Endpoint", preset.endpoint())?;

    // The key is settled *before* the model, which is the whole reason the
    // endpoint can be asked what it serves: a listing needs authenticating.
    let key = ask_key(
        console,
        deps.store,
        deps.env_is_set,
        pending,
        preset,
        &endpoint,
    )?;
    let model = ask_model(
        console,
        deps.source,
        preset,
        &endpoint,
        key.usable.as_deref(),
    )
    .await?;

    // The chosen model, not the preset, decides `temperature` and `max_tokens`.
    // Nothing here can fail: an absent registry, or one that does not name this
    // model, yields the preset's values unchanged.
    let quirks = quirks::resolve(
        registry.get(console, deps.quirks_source).await,
        preset.quirks(),
        &endpoint,
        &model,
    );
    let endpoint_for_store = endpoint.clone();

    Ok((
        Choice::http(preset, model, endpoint, key.in_store, quirks),
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
    env_is_set: &dyn Fn(&str) -> bool,
    pending: &[(String, String)],
    preset: &LlmPreset,
    endpoint: &str,
) -> Result<KeyChoice> {
    // A preset that needs no key at all - a local server - has nothing to ask.
    // `usable` stays `None`, and the listing is attempted unauthenticated,
    // which is what such a server expects.
    let Some(env) = preset.api_key_env() else {
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

    if let Some(url) = preset.key_url() {
        console.say(&format!("  Get a key: {url}"))?;
    }

    // Reported because it changes what the empty answer means: with the
    // variable already exported, skipping is a complete setup rather than a
    // deferred one.
    if env_is_set(env) {
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
            // Read directly rather than through `env_is_set`, which answers
            // only whether a value exists. A test injecting a stub reports no
            // usable key here, which is what it should: there is no value to
            // authenticate a listing with.
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

mod console;
pub use console::{Console, Terminal};

#[cfg(test)]
pub(crate) mod tests;
