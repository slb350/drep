//! A [`Console`](super::super::Console) driven by a script instead of a terminal.

use anyhow::{Result, anyhow};

use super::super::Console;

/// Answers the wizard's questions from a queue, recording everything it said.
///
/// Lines and secrets come from the *same* queue, deliberately: the order the
/// wizard asks in is part of what these tests pin, and separate queues would
/// let a reordering pass unnoticed.
pub(crate) struct Scripted {
    answers: std::collections::VecDeque<String>,
    /// Everything printed, including the questions, one entry per call.
    pub said: Vec<String>,
    /// Which questions were asked as secrets, in order.
    pub secrets_asked: Vec<String>,
}

impl Scripted {
    /// A console that will answer with `answers`, in order.
    pub fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|a| (*a).to_string()).collect(),
            said: Vec::new(),
            secrets_asked: Vec::new(),
        }
    }

    /// Everything the wizard printed or asked, joined for substring assertions.
    pub fn transcript(&self) -> String {
        self.said.join("\n")
    }

    /// Whether every scripted answer was consumed.
    ///
    /// A test that leaves answers behind is asserting against a flow that did
    /// not happen, so this is checked rather than assumed.
    pub fn is_drained(&self) -> bool {
        self.answers.is_empty()
    }

    fn next(&mut self, question: &str) -> Result<String> {
        self.answers
            .pop_front()
            .ok_or_else(|| anyhow!("the wizard asked more than the script answered: {question}"))
    }
}

impl Console for Scripted {
    fn say(&mut self, line: &str) -> Result<()> {
        self.said.push(line.to_string());
        Ok(())
    }

    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<String> {
        self.said.push(match default {
            Some(value) => format!("{question} [{value}]"),
            None => question.to_string(),
        });
        let answer = self.next(question)?;
        // The real terminal substitutes the default for an empty line, so the
        // stand-in has to as well or every "press Enter" case would diverge.
        Ok(match (answer.trim().is_empty(), default) {
            (true, Some(value)) => value.to_string(),
            _ => answer,
        })
    }

    fn ask_secret(&mut self, question: &str) -> Result<String> {
        self.said.push(question.to_string());
        self.secrets_asked.push(question.to_string());
        self.next(question)
    }
}

/// A [`ModelSource`](crate::llm::models::ModelSource) driven by a canned answer.
///
/// The wizard's model prompt is the one step that talks to a network, so the
/// tests inject this instead: a real source would make the suite slow, offline-
/// hostile, and dependent on somebody's plan still including a given model.
pub(crate) enum Catalog {
    /// The endpoint serves these, in this order.
    Serves(Vec<crate::llm::models::Model>),
    /// The endpoint has no listing route - a local server, a gateway, anything
    /// older. The wizard must fall back to typing a name.
    Unavailable,
    /// The endpoint rejected the key.
    Rejected,
}

impl Catalog {
    /// A catalogue of the given ids, with no display names.
    pub fn of(ids: &[&str]) -> Self {
        Self::Serves(
            ids.iter()
                .map(|id| crate::llm::models::Model {
                    id: (*id).to_string(),
                    display_name: None,
                })
                .collect(),
        )
    }
}

impl crate::llm::models::ModelSource for Catalog {
    async fn list(
        &self,
        _endpoint: &str,
        _api_key: &str,
        _protocol: open_agent::ApiProtocol,
    ) -> Result<Vec<crate::llm::models::Model>, crate::llm::models::ListError> {
        match self {
            Self::Serves(models) => Ok(models.clone()),
            Self::Unavailable => Err(crate::llm::models::ListError::Unsupported),
            Self::Rejected => Err(crate::llm::models::ListError::Unauthorized(401)),
        }
    }
}

/// A catalogue that records what it was asked, so a test can assert the key
/// reached the listing rather than an empty string.
pub(crate) struct Recording {
    inner: Catalog,
    /// `(endpoint, key, protocol)` per call, in order.
    pub calls: std::cell::RefCell<Vec<(String, String, open_agent::ApiProtocol)>>,
}

impl Recording {
    pub fn new(inner: Catalog) -> Self {
        Self {
            inner,
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl crate::llm::models::ModelSource for Recording {
    async fn list(
        &self,
        endpoint: &str,
        api_key: &str,
        protocol: open_agent::ApiProtocol,
    ) -> Result<Vec<crate::llm::models::Model>, crate::llm::models::ListError> {
        self.calls
            .borrow_mut()
            .push((endpoint.to_string(), api_key.to_string(), protocol));
        self.inner.list(endpoint, api_key, protocol).await
    }
}

/// The number the wizard prints beside `key`, as a string to answer with.
///
/// Computed from `PRESETS` rather than written out, so adding a preset shifts
/// every test's answer automatically instead of silently selecting the wrong
/// provider.
pub(crate) fn number_of(key: &str) -> String {
    let index = crate::cli::init::presets::PRESETS
        .iter()
        .position(|preset| preset.key == key)
        .unwrap_or_else(|| panic!("no preset named `{key}`"));
    (index + 1).to_string()
}

/// A [`QuirksSource`](crate::llm::quirks::QuirksSource) driven by a canned
/// registry.
///
/// Built from a models.dev-shaped JSON literal rather than from a hand-made
/// `Registry`, so the distillation the wizard depends on is the one under test
/// here too - and so no test in this crate reaches models.dev.
pub(crate) enum Quirked {
    /// models.dev could not be reached and no cache could stand in for it.
    Unavailable,
    /// The registry drep would have distilled from this document.
    Knows(crate::llm::quirks::Registry),
}

impl Quirked {
    /// A registry distilled from a models.dev-shaped document.
    pub fn from_json(body: &str) -> Self {
        Self::Knows(
            crate::llm::quirks::Registry::distil(body, 0).expect("the fixture document distils"),
        )
    }
}

impl crate::llm::quirks::QuirksSource for Quirked {
    async fn registry(
        &self,
    ) -> Result<crate::llm::quirks::Registry, crate::llm::quirks::QuirksError> {
        match self {
            Self::Unavailable => Err(crate::llm::quirks::QuirksError::Transport(
                "the stub is offline".to_string(),
            )),
            Self::Knows(registry) => Ok(registry.clone()),
        }
    }
}

/// An environment lookup answering "not set" for every variable.
///
/// A `static` rather than a closure at each call site, so the reference handed
/// to `Deps` outlives the call and every test states the same condition the
/// same way. The lookup is injected in the first place because
/// `std::env::set_var` is `unsafe` in edition 2024 - a concurrent reader on
/// another thread is a data race, and `cargo test` is multi-threaded.
pub(crate) static NEVER_SET: fn(&str) -> bool = |_| false;

/// An environment lookup answering "set" for every variable.
pub(crate) static ALWAYS_SET: fn(&str) -> bool = |_| true;

/// A redacted, deterministic Codex diagnostic for wizard tests.
pub(crate) static CODEX_READY: fn() -> Result<crate::llm::codex::CodexStatus, String> =
    || Ok(crate::llm::codex::CodexStatus::new("test-version"));

/// The wizard's dependencies for a test: no network, no process environment,
/// and whatever credential store the caller supplies.
///
/// Written out at each call site, the four fields were spelled thirteen times,
/// eleven of them identical but for one. Adding a field to `Deps` then means
/// editing thirteen unrelated tests, which is what happened when the quirks
/// source arrived.
pub(crate) fn deps<'a, S, Q>(
    store: &'a crate::auth::AuthStore,
    source: &'a S,
    quirks_source: &'a Q,
) -> super::super::Deps<'a, S, Q> {
    super::super::Deps {
        store,
        source,
        quirks_source,
        env_is_set: &NEVER_SET,
        codex_status: &CODEX_READY,
    }
}

/// [`deps`], with every environment variable reported as already set.
pub(crate) fn deps_with_env_set<'a, S, Q>(
    store: &'a crate::auth::AuthStore,
    source: &'a S,
    quirks_source: &'a Q,
) -> super::super::Deps<'a, S, Q> {
    super::super::Deps {
        env_is_set: &ALWAYS_SET,
        ..deps(store, source, quirks_source)
    }
}
