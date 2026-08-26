//! What the semantic layer is, for this repository: an analyzer, or a refusal.
//!
//! One question answered in one place, and the order it is answered in is the
//! feature. Site policy is probed **first**, before the credential store is
//! opened, before any `api_key_command` runs and before [`ProviderChain::new`] -
//! which for a `codex` entry spawns the Codex CLI to read ChatGPT login state.
//! Building the chain ahead of the refusal would start the model machinery for a
//! repository whose source must never reach a model, and minting a short-lived
//! credential would put a request to the gateway ahead of the decision not to
//! use it.
//!
//! The two-arm [`Source`] is what makes "refused implies no chain" structural
//! rather than a convention the orchestrator has to keep. There is no arm holding
//! both, so no later code can consult a provider for a refused repository by
//! forgetting a branch.
//!
//! Input resolution runs ahead of the probe, because the question is which
//! repositories this run would send source *from* - see
//! [`reviewed_directories`]. It reads local files and contacts nothing, so it
//! comes before everything the ordering above is about.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::auth;
use crate::cli::MachineFiles;
use crate::config::site::{Refusal, SiteConfig};
use crate::config::{self, Config};
use crate::llm::cache::Cache;
use crate::llm::chain::ProviderChain;

use super::input::Work;

/// Where the files this resolution reads live.
///
/// Grouped rather than passed one by one: the repository's config and the two
/// machine files travel together everywhere in `check`, and all of them are
/// parameters precisely so no test reads real machine state.
pub(super) struct Locations<'a> {
    pub(super) config: &'a Path,
    pub(super) machine: &'a MachineFiles<'a>,
}

/// The semantic layer for this run.
pub(super) enum Source {
    /// Site policy refuses review here. No provider was contacted, no
    /// credential was resolved, and no cache entry was read or written.
    Refused(Refusal),
    /// Review is permitted, and this is what will do it.
    Analyze(CodeQualityAnalyzer),
}

/// Probe the policy, and build the analyzer only if it permits one.
///
/// `config` is taken by mutable reference because [`auth::resolve`] fills in the
/// keys the file left unset. A refused repository never reaches that call, so its
/// config keeps whatever `drep.toml` said - which is correct: nothing is going to
/// use a credential here.
pub(super) async fn source(
    locations: &Locations<'_>,
    config: &mut Config,
    site: Option<&SiteConfig>,
    work: &Work,
    cache: Cache,
    cache_only: bool,
) -> Result<Source> {
    // `has_refuse_markers` was asked before input resolution, so a fleet policy
    // that sets only `max_concurrent_ceiling` did not build this directory set.
    // The guard stays here too because `Work` is constructible in tests and an
    // empty marker list should never turn an absent scope into a policy error.
    if let Some(site) = site
        && site.has_refuse_markers()
        && let Some(refusal) = site
            .refusal_among(&work.reviewed_directories, locations.machine.policy)
            .await?
    {
        return Ok(Source::Refused(refusal));
    }

    // Fill in the keys the file left unset from the user-level store, and run any
    // `api_key_command` an entry declares. An explicit `api_key` in `drep.toml`
    // always wins, so this cannot change what an existing config does; it only
    // supplies what `drep init` stopped writing into the repository. A store that
    // cannot be read is fatal rather than treated as empty - running the gate
    // unauthenticated would surface as a 401 per file, which reads as a broken key
    // rather than a broken store. A failing `api_key_command` is fatal here for
    // the same reason and one more: the chain does not exist yet, so there is
    // nothing to fail over to, and routing around a broken credential path is
    // what hides it.
    let store = auth::AuthStore::load(locations.machine.auth).with_context(|| {
        format!(
            "could not read the auth store at {}",
            locations.machine.auth.display()
        )
    })?;
    auth::resolve(config, &store).await?;
    // The whole enabled chain, not just its head: `providers()` is the
    // failover order, and `load` has already rejected a config with none.
    // The `is_empty` guard stays because `Config` is constructible without
    // going through `load`, and a panic inside the commit gate is a worse
    // failure than a message naming the file. The error is `load`'s own rather
    // than a second sentence written here: two hand-written copies of "no
    // provider configured" drift, and the copy that lived here had already
    // lost the actionable "run `drep init`" half.
    let providers = config.providers();
    if providers.is_empty() {
        return Err(config::ConfigError::NoProviders(locations.config.to_path_buf()).into());
    }
    let chain =
        ProviderChain::new(&providers).map_err(|e| anyhow!("could not build LLM analyzer: {e}"))?;
    Ok(Source::Analyze(
        CodeQualityAnalyzer::new(chain, cache).with_cache_only(cache_only),
    ))
}
