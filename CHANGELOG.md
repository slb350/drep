# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.6.0] - 2026-08-22

### Added

- Authoritative semantic checks now enforce a three-round remediation budget
  per branch and worktree. A round counts only when a fresh provider response
  still contains an actionable finding after compiler-grounded suppression and
  acknowledgements. Cached verdicts and deterministic tools remain available
  at the limit; a cold fourth review exits 2 without contacting a provider.
- `max_review_rounds` configures the default, while
  `--max-review-rounds N` and `--unlimited-reviews` explicitly authorize a
  longer cycle. Text and JSON output report counted, reset, unlimited and
  limit-reached states.

### Changed

- All validation and release jobs now run on repository-scoped homelab
  runners: Linux and global release work use the hardened Strix service, while
  native macOS validation and both Apple release targets use the Xcode-equipped
  M1 Mac mini. Forked pull requests cannot execute on either LAN host, and the
  cargo-dist release workflow is tag-only. Strix's quick Linux gates share one
  runner allocation, and the full mutation sweep starts only after successful
  main-push validation.
- Clean complete diff and push checks reset the remediation cycle. Clean
  staged subsets and named-path checks do not erase full-branch accounting;
  clean responses, acknowledged or compiler-disproved findings, and pure
  analysis failures refund their pending reservation.
- Review-round state is stored outside the response cache under worktree-local
  Git metadata. Atomic slot claims prevent concurrent oversubscription, and
  stale pending or incomplete claims recover after a killed process.

## [2.5.1] - 2026-08-22

### Changed

- Refreshed the locked Rust dependency graph to the newest releases compatible
  with the declared Rust 1.88 minimum, including direct runtime dependencies
  `blake3` and `ignore` plus their HTTP, Unicode, and build-time transitive
  dependencies.
- Full mutation CI now runs on the repository-scoped Strix GitHub Actions
  runner for pushes to `main` only. Hosted Linux, macOS, lint and MSRV lanes
  remain unchanged; the duplicate multi-hour hosted mutation job is gone.
  The persistent runner keeps only `target/` between sweeps and fails closed
  on any other ignored or untracked workspace state. Its dedicated no-login
  account is isolated from user homes and private-network egress, and every
  external contributor's fork workflow requires maintainer approval. The job
  retains a 90-minute stuck-run ceiling, with enough headroom for the measured
  full sweep to finish when Strix is serving another repository concurrently.
- Source-file discovery now delegates directly to the allocation-free language
  registry lookup instead of allocating lowercase and dotted extension strings
  for every path visited by the repository walk.

## [2.5.0] - 2026-08-21

### Added

- `drep check --pre-commit-push` adapts pre-commit's pre-push environment to
  drep's diff input. The published hook disables filename arguments and reviews
  the exact `PRE_COMMIT_FROM_REF...PRE_COMMIT_TO_REF` hunks; pre-commit's
  explicit all-files case for a new root branch remains all-files.

### Changed

- Semantic review now asks only for concrete, reachable defects worth fixing
  before merge. Optional hardening, implausible extreme edge cases, nits,
  subjective preferences, cleanup, and speculative findings are explicitly out
  of scope. Autonomous remediation should stop after three LLM-driven fix
  rounds by default and hand remaining advisory findings to a person; drep
  deliberately keeps reviewing so a round counter cannot hide a new defect.
  Because the system prompt is part of the response-cache identity, upgrading
  intentionally starts this policy with cold semantic-review entries.

### Fixed

- The published pre-commit pre-push hook no longer passes changed filenames to
  `drep check`. Filename input selected whole-file mode, so a small follow-up
  fix caused the model to reconsider unchanged code around it instead of
  receiving the same diff-hunk scope as drep's native hook.

## [2.4.0] - 2026-08-21

### Added

- LLM findings now carry a source-sensitive fingerprint. `drep acknowledge
  <fingerprint>` records a reviewed false positive in
  `.drep/acknowledgements.toml`; the finding stays suppressed while its file,
  category and surrounding source are unchanged, and expires automatically
  when that code changes.

### Changed

- Deterministic tools are planned per nearest configured ancestor. Workspace
  members can own their own `eslint.config.*`, `tsconfig.json`, `Cargo.toml`,
  `pyproject.toml`, or other supported config while still resolving a
  repository-hoisted executable. `tsc` now runs the configured project rather
  than receiving file arguments that make it ignore `tsconfig.json`. Tool
  fan-out is capped at four processes and repository clippy tasks are
  serialized to avoid creating Cargo lock contention.
- Clippy allows up to 30 minutes for Cargo's own build-directory lock and names
  that lock wait if the extended ceiling is exhausted. Other deterministic
  tools retain the two-minute ceiling.
- A non-empty unparseable model response is attempted three times, then handed
  to the next configured provider. This is per-file recovery: it does not
  demote the provider for later files.

### Fixed

- LLM responses explicitly identify findings that claim compilation failure.
  When clippy, tsc, or go vet succeeds for that same file in the same check,
  drep suppresses the disproved claim while retaining semantic findings.

- Pre-push reviews no longer resume a remote connection that may have sat idle
  for the whole LLM run. The generated and published hooks use a cache-first
  push gate: a cold review completes and caches, exits 3 with an explicit
  `git push` retry instruction, and the immediate retry reconnects and uses the
  cached verdict. `drep check --cache-only` exposes the no-network lookup.
- Hook installation now distinguishes drep-managed scripts by a header marker,
  resolves the active hooks directory before writing anything, refreshes stale
  managed chainers, and never treats a comment mentioning a hook path as an
  active chainer. Cache entries are published through a unique sibling file and
  atomically replace the destination, so concurrent writers cannot expose
  partial JSON or follow a planted destination symlink.
- Push-gate retries preserve cache misses for files outside the retried diff,
  multi-ref hooks retain the semantic status order `2 > 1 > 3 > 0`, and empty
  provider-chain failures render a stable diagnostic instead of assuming an
  attempt exists.
- Generated pre-push hooks fail closed when drep exits with an unknown nonzero
  status. Analyzer calls that accidentally mix files are partitioned before
  review, so findings cannot be attributed to the first path in release builds.
  Foreign hooks are classified byte-safely, backed up byte-for-byte under
  `--force`, and an existing recovery backup is never overwritten.

## [2.3.0] - 2026-08-20

### Added

- **ChatGPT/Codex subscription review is a first-class backend, separate from
  the OpenAI API.** `drep init --provider codex` writes an endpoint-less,
  keyless `backend = "codex"` entry and invokes a separately installed Codex
  CLI with ChatGPT authentication forced. Each review is ephemeral and runs in
  an empty directory with a read-only sandbox, no approvals, ignored user and
  project configuration, an allowlisted environment, disabled tool surfaces,
  bounded output and a strict response schema. The JSONL event parser rejects
  any command, file, MCP, web or subagent activity as a contract violation and
  surfaces terminal error events without guessing their class from prose.
- `drep doctor` reports the Codex CLI version, ChatGPT-managed authentication
  and isolation mode without retaining or printing account details. The
  interactive wizard checks the same readiness before producing a plan;
  `drep auth --provider codex` points to `codex login` and never mutates drep's
  API-key store.
- HTTP and Codex providers now share a backend boundary, so either can sit in a
  failover chain. Cache identity separates HTTP endpoint/protocol from Codex
  CLI version, ChatGPT auth mode and reasoning effort, including when both
  backends use the same model name. Existing HTTP cache entries miss once
  because their backend identity now carries an explicit `http:` namespace.

### Changed

- The existing `openai` preset is now labelled **OpenAI API** to make its
  per-token API billing distinct from ChatGPT/Codex subscription allowance.
  Its endpoint, model, key variable and Chat Completions wire contract are
  unchanged and covered by an OpenAI-specific mock-server test.

### Fixed

- Independent Kimi review hardened the Codex subprocess boundary, including
  bounded stdout and stderr capture, child cleanup, event validation and
  diagnostic redaction. It also tightened atomic config publication, cache
  ownership and eviction, terminal finish handling, configuration validation,
  and the remote mutation-sweep argument contract.

## [2.2.0] - 2026-08-20

### Added

- **`drep init` writes `temperature` and `max_tokens` for the model you picked,
  not for its provider.** Both are properties of a model, and the presets held
  one value each: `kimi` sent no temperature because `k3` refuses one, and a
  required `max_tokens` of 200,000 because that is a number the endpoint
  accepts. The wizard now consults a distilled, weekly-refreshed copy of
  [models.dev](https://models.dev) - the one index publishing `temperature` and
  `limit.output` per model - so `k3` gets its own 131,072 and
  `kimi-for-coding` gets 32,768, where one provider-scoped number could not
  have been right for both. `GET /models` cannot answer this: it carries ids
  and nothing else.

  The registry only ever narrows. It may withdraw a `temperature` and may lower
  a *required* `max_tokens`; it never introduces either, because sending a
  parameter drep would have omitted is the direction that produces a 400, and a
  400 neither fails over nor retries. An index that disagrees with an endpoint
  therefore cannot break a provider that worked before this existed.

  Nothing about it can stop `drep init`. A missing, unreadable, unparseable or
  wrongly-shaped cache means "refetch"; a failed fetch falls back to a stale
  copy, or to the preset's own values; a model the registry does not name keeps
  the preset's values, which is what `drep init` wrote before. `--provider`
  runs do not consult it at all, so a scripted setup still needs no network.
  `DREP_QUIRKS_PATH` relocates the cache, which otherwise sits beside
  `auth.toml`.

  The rendered `drep.toml` says which of the two it wrote, because "this is the
  model's own limit" is a claim in a file you commit.

### Changed

- **open-agent-sdk 0.10.0.** Streamed text now reaches drep one `StreamEvent`
  per delta while the stream is open, where 0.9.x concatenated the whole
  response into a single block at its end. `run_one_query` already appended
  every text block it received, so the assembled response is byte-identical and
  nothing failed to compile - which is the hazard, since the types are
  unchanged and only the event count differs. `src/llm/client/tests/streaming.rs`
  now delivers the same bytes as one delta and as nine, splitting inside a JSON
  key and inside a number, and asserts the extracted value is the same; it
  fails against a client that keeps the first block, keeps the last, or joins
  the fragments with a separator.
- `a_response_with_no_finish_reason_is_retried` finally tests what it is named.
  Its fixture reported `"stop"`, because under 0.9.x a stream that never sent a
  `finish_reason` yielded no text at all and the case could not be written.
  0.10.0 delivers the text and finishes as `Unspecified`, so
  `test_support::sse_without_finish_reason` builds the real thing.
- `auth::ensure_dir_private` is now the single definition of "create this
  directory, and narrow it to 0700 only if drep made it", shared with the
  quirks cache. The two files sit in the same directory, so a second copy that
  only called `create_dir_all` would have left the credential store's own
  directory world-readable whenever the cache happened to be written first.
- The registry document is fetched compressed. models.dev serves it gzipped at
  399 KB against 4.01 MB uncompressed, and `reqwest` was built without the
  feature, so drep asked for the whole thing. On a 2 Mbit link that is 16
  seconds of an interactive command against a 20-second timeout. One
  consequence is deliberate: `reqwest` strips `Content-Length` from a response
  it decodes, so the header check is now a shortcut and the streaming cap is
  the bound - and it counts decoded bytes, which is what gets allocated.
- `crate::http` holds the one bounded GET. `drep init` makes two plain requests
  of its own, for the model listing and the registry document, and each had
  written its own client and body read.
- One models.dev fixture, in `test_support`, instead of one per suite. The two
  copies disagreed about whether `glm-5.3` accepts a temperature, so neither
  was readable as the fixture's claim. It now refuses one and `glm-5.2`
  accepts, which puts both directions on the same endpoint.

### Fixed

- **The registry could widen a fact instead of narrowing it.** Two providers
  may publish the same `api` URL, and nothing says their model lists are
  disjoint - models.dev ships `minimax` and `minimax-coding-plan` at one
  endpoint. Duplicate entries were merged last-wins, and an omitted field
  defaults to the permissive answer (`temperature: true`, no limit), so a
  sparse entry landing second re-introduced a parameter the model rejects.
  Which entry lands second is an accident of the vendor id's sort order. Facts
  now narrow field by field, which is the one guarantee the registry makes.
- **The credential store was written in place.** `auth.toml` was opened with
  `truncate`, so a crash, a full disk or a serialization failure between that
  and the last byte left it empty or half-written - and it is the one file drep
  holds that cannot be regenerated. It is now written to a sibling created 0600,
  synced, and renamed over the target.
- **`auth::normalise` lowercased the path of an endpoint with no scheme.** Its
  own rule is that a URL path is case-sensitive, and the scheme-carrying branch
  followed it; `localhost:11434/V1` did not, which is the spelling a local
  server is most likely to be typed as. Two such endpoints collapsed onto one
  entry, and one of them got the other's key.
- **Writing `drep.toml` followed a dangling symlink.** `Path::exists` reports
  false for a symlink whose target is missing while `fs::write` creates that
  target, so the "refuse to overwrite" guard saw nothing there and the config
  went to a path nobody named. The write is now one `create_new` call, which
  also closes the gap between the check and the write.
- The git environment scrub covers `GIT_OBJECT_DIRECTORY`,
  `GIT_ALTERNATE_OBJECT_DIRECTORIES` and `GIT_QUARANTINE_PATH` as well. They
  redirect where a child `git` reads and writes objects, so an inherited one
  points at the outer repository while every other setting names the intended
  one.
- **The model listing was read without a size ceiling.** `drep init` asks an
  endpoint the user has just typed which models it serves, holding a key while
  it does, and buffered whatever came back. The registry fetch written beside
  it had a ceiling and a chunked read from the start; the older listing call
  kept `text()`. Both now go through `crate::http::read_bounded`, which is the
  point of having one of it.
- **A rendered `drep.toml` claimed a model's output limit was unknown while
  printing it.** The comment above `max_tokens` is decided by whether the
  registry named the model's own limit, and that was read off `limit <
  fallback` - so a model publishing exactly the preset's fallback got the
  "not known here" wording for a number the registry had named.
- A failed hook rename left its temporary file behind. `drep init` is a command
  people re-run, so a repeatedly-failing install accumulated one file per
  attempt in `.git/hooks`. The quirks cache already cleaned up after itself.
- **`the_required_max_tokens_leaves_headroom_against_the_models_window`
  asserted something untrue.** `k3` publishes an output ceiling of 131,072 and
  `kimi-for-coding` 32,768, so the preset's 200,000 was above both rather than
  below either. The value stays - the endpoint is verified to accept it, and it
  is now only the fallback for a model the registry cannot name - and the test
  says what it actually pins.

## [2.1.0] - 2026-08-20

### Added

- **`drep init` is now interactive.** Run it with no `--provider` on a terminal
  and it asks: which provider (the preset table, with descriptions), which
  model and endpoint, where to get a key and then the key itself, whether to add
  a fallback provider, which hooks, and whether to gitignore the config.
  `--provider` still takes the scripted path unchanged; `--non-interactive`
  forces it, and `--interactive` forces the wizard where stdin is a pipe.
- **Keys are stored per machine instead of being exported by hand.** A pasted
  key goes to `~/.config/drep/auth.toml` (macOS:
  `~/Library/Application Support/dev.slb350.drep`) at mode 0600, keyed by
  endpoint, and the rendered `drep.toml` then carries no `api_key` line at all.
  `api_key = "${VAR}"` still works and still wins over the store, which is what
  CI needs. `DREP_AUTH_PATH` relocates the store.
- **`drep auth list` / `login` / `logout`**, for rotating a key, adding one for
  a hand-edited endpoint, and checking what is held. None of them prints a key.
- **`drep init` adds `drep.toml` to `.gitignore`** by default, asking first in
  the wizard. `--no-gitignore` opts out. It asks git rather than reading the
  file, so an existing rule (including a glob) is recognised, and a `drep.toml`
  that is already *tracked* is reported with the `git rm --cached` fix instead
  of being silently appended to a file that cannot affect it.
- **`doctor` reports where each provider's key comes from** - the config, the
  store, or nowhere.
- **The wizard writes a failover chain**, not just one provider, so the
  local-first/cloud-fallback pairing the chain was built for is reachable
  without hand-editing the file.
- **The wizard offers the models the endpoint actually serves.** After the key
  is entered, `drep init` asks `GET {endpoint}/models` and lists what came back,
  preselecting the preset's default when the endpoint still offers it and saying
  so when it does not. A model name outside the list is still accepted, and an
  endpoint with no listing route falls back to typing one exactly as before.
  This replaces a hardcoded default that nothing checked, where a typo or a
  model outside your plan surfaced as a 404 on the first push.
- **Re-running `drep init` on a configured repository now offers to replace
  it**, printing what is currently configured first and defaulting to no. That
  is how you switch providers; `drep auth login` rotates a key without touching
  the config. Non-interactive runs still refuse and name `--force`.

### Fixed

- **A second `drep init` half-applied itself.** The existing-config check ran at
  the config *write*, which is after the wizard has already stored a pasted key.
  So the second run asked every question, saved the credential, failed on
  "drep.toml already exists" and exited 0 - leaving the store changed, the
  config untouched and the provider not switched. The decision now happens
  before the first question.

- **Three subscription-plan providers, and a second wire protocol to reach two
  of them.** `drep init` gains `zai`, `minimax` and `kimi` presets alongside the
  existing four. Verified against the live endpoints on 2026-08-19: z.ai
  `glm-5.3` returned 7 findings in 32.6s,
  MiniMax `MiniMax-M3` 7 findings in 6.0s, Moonshot `k3` 6 findings in 28.7s.
  All three found the `eval()` on file contents and rated it `error`.
- **`protocol = "anthropic"`** on an `[[llm]]` block, for endpoints exposing the
  messages API rather than chat completions. Kimi for Coding and MiniMax publish
  their subscription tiers only that way. The default is `openai`, so no existing
  file changes. `doctor` tags a non-default protocol in its provider listing.
- **open-agent-sdk 0.9.0** carries the protocol itself: request path, auth
  header, body translation and streaming vocabulary, with extended thinking
  routed to the reasoning channel the SDK already had. Depended on from
  crates.io; it was briefly a path dependency during development, which also
  broke the remote mutation sweep, since `strix.local` cannot resolve a path
  that exists only on one machine.

### Changed

- **`temperature` is now `Option<f32>`; an absent value sends no temperature at
  all** rather than defaulting to 0.2. Two of the four models with presets
  reject the parameter outright — `k3` answers `only temperature 1 is allowed
  for this model`, `gpt-5.6-sol` refuses any value — and a 400 neither fails
  over nor retries, so "send none" had to be expressible. `drep.toml` in this
  repository now writes `temperature = 0.2` explicitly.
- **The cache key includes the protocol.** `api.minimax.io` serves `MiniMax-M3`
  over both `/v1` and `/anthropic/v1`; keying without it files one protocol's
  answer where the other looks for its own. Existing entries miss and re-run
  once.
- `json_parsing`'s inline test module was split into
  `src/llm/json_parsing/tests/` (four files, verbatim) as the file approached
  the 600-line limit.

### Fixed

- **A response that was already JSON could be mangled by a fence *inside* it.**
  The extraction ladder tried the fence strategy first, so a valid JSON answer
  whose finding text quoted "```" had `working` replaced by that inner fence's
  body, and every later strategy then ran on prose - reporting `Unparseable` for
  an answer that was JSON from the first character. The whole response is now
  parsed before any fence is looked for. Found by drep's own pre-push gate,
  reviewing `json_parsing.rs`, which is the file that does this.
- **`drep auth`'s store no longer chmods a directory it did not create.**
  `save` narrowed the parent to 0700 unconditionally, so a `DREP_AUTH_PATH`
  under `/etc` would have taken `/etc` with it. Only a directory drep creates is
  narrowed; the file's own 0600 is what guards the key.
- **The store file is created 0600 rather than written and then chmodded**, so
  the key is never briefly world-readable, and a key containing a control
  character is refused at the prompt instead of failing on the first request.
- **Endpoint normalisation no longer lowercases the URL path.** Scheme and host
  are case-insensitive; a path is not, so `/API/v1` and `/api/v1` on one host
  collapsed onto a single entry and could hand one endpoint's key to the other.
- **`doctor` never prints a literal `api_key`.** A `${VAR}` reference is echoed,
  because showing which variable is the point; a literal value is reported as
  present without its contents, since that output is what gets pasted into bug
  reports and CI logs.
- **`wizard::Plan` hand-writes `Debug`**, having held pasted keys behind a
  derived one.
- **`temperature` is rendered with `{:?}`**: `Display` writes `1.0` as `1`,
  which TOML reads as an integer and `config::load` then rejects - from a file
  `drep init` had just reported writing.
- **The cache key formats temperature as a shortest round-trip float.** Six
  decimal places is coarser than `f32`'s resolution near 1.0, so two distinct
  temperatures could share a key.
- **Answering "Replace drep.toml?" no longer authorises overwriting a foreign
  git hook.** The interactive answer applies to the config it asked about;
  `--force` remains the only thing that clobbers a hook drep did not write.
- **No test writes to the process environment.** `std::env::set_var` is `unsafe`
  in edition 2024 because a concurrent reader is a data race, and `cargo test`
  is multi-threaded - the three call sites' "single-threaded test process"
  safety comments were simply untrue. The auth path and the wizard's
  environment lookup are now injected instead.

- **A leading `<think>...</think>` block is stripped before the JSON extraction
  ladder runs.** MiniMax's M-series over its OpenAI-compatible endpoint, and
  most local llama.cpp and MLX builds of Qwen, emit the whole reasoning trace
  inline in `message.content`. Deliberation about a code review quotes code, so
  that trace carries a fenced block of its own, and `FENCE_RE` takes the first
  fence — the ladder selected the reasoning's sample, every later strategy
  failed on it, and the file came back `Unparseable`, which by design neither
  fails over nor retries. Every file would have failed with the fallback never
  consulted.
- `api.kimi.com/coding/v1` requires `max_tokens` and answers a bare
  `invalid_request_error` 400 without it, which names no field. The `kimi`
  preset sets 200,000; the rule that presets carry no cap otherwise is now
  asserted as an exact exception list rather than a blanket prohibition.

### Fixed

- The declared MSRV is 1.88. `ignore` 0.4.30 uses let-chains without declaring
  its own `rust-version`; measured builds fail on 1.86 and 1.87 and pass on
  1.88. Pinning the dependency back would freeze the file walk. Raising the
  floor also enabled the matching clippy lints in the source and tests.
- `two_endpoints_serving_the_same_model_do_not_share_a_cache_entry` failed on
  Linux and passed on macOS. Run one's mock servers were dropped at the end of
  a block, which frees their ephemeral ports, and Linux hands the next listener
  a port it just released - so the "revived head" came up on the address the
  fallback had used, the two endpoints were the same string, and the keys
  matched. The servers now live for the whole test, and it asserts the two
  endpoints differ before comparing keys. Nothing was wrong with the cache key.
  The same failure took the mutants job with it, as a failing baseline.
- The first full mutation sweep in CI found a survivor: deleting the `!` from
  `!missing.contains(&spec.name)` in `doctor::write_tools_section` made the
  missing-tools list permanently empty, and no test noticed. Both tests that
  cover it branch on whether a real binary happens to be installed on the
  machine, and both take their "nothing is missing" path when it is - the same
  path the broken version takes, which is why it passed on a runner that has
  `tsc`. Two tests now ask the question with no dependency on the machine,
  against a language whose tool is a command name nothing can have: one for the
  list being populated, one for a tool shared by two languages appearing once.
  Verified by applying the mutation by hand and watching them fail.
- The v2.0.0 release built and published every binary, then failed to push the
  Homebrew formula. The `publish-homebrew-formula` job passes the
  whole `dist plan` manifest to the runner as one environment variable, Linux
  caps a single environment variable at 128 KB (`MAX_ARG_STRLEN`), and the
  release manifest exceeded it because cargo-dist embeds the changelog section
  twice. Release entries now stay concise so the manifest remains below the
  process limit.

## [2.0.0] - 2026-08-19

drep is a Rust binary, and the scope is much smaller. It runs the linters and
formatters your repository already configures, sends the code you changed to an
LLM for review, and gates commits and pushes on the result. That is all it does.

### Added

- A single binary for macOS and Linux, x86_64 and arm64. Install with the shell
  installer from the release, or `brew install slb350/tap/drep`.
- `drep init` writes `drep.toml` and native git hooks, handling `core.hooksPath`
  so a repository-local hook is not silently ignored.
- A failover chain: `[[llm]]` is an ordered array, and a timeout, refused
  connection, 429, 5xx or empty answer falls through to the next provider. A 401
  does not, because that is a broken key.
- `drep lint-docs`, ten rule-based markdown checks with no LLM and no network,
  gated by `--fail-on <severity>`.
- `drep doctor` reports which languages, tools and providers are visible.

### Changed

- Deterministic tool findings gate; LLM findings inform unless `--fail-on` opts
  them in. Splitting by source rather than severity is what makes the gate
  calibratable.
- Exit 2 means something that should have run did not: an unreachable endpoint,
  a file too large for the model, a configured tool that is not installed. It is
  never reported as a pass.
- Configuration is `drep.toml`. `api_key` names an environment variable rather
  than holding a secret.
- Python, JavaScript, TypeScript, Go and Rust are all first-class. Adding a
  language is an entry in one table.

[Unreleased]: https://github.com/slb350/drep/compare/v2.6.0...HEAD
[2.6.0]: https://github.com/slb350/drep/compare/v2.5.1...v2.6.0
[2.5.1]: https://github.com/slb350/drep/compare/v2.5.0...v2.5.1
[2.5.0]: https://github.com/slb350/drep/compare/v2.4.0...v2.5.0
[2.4.0]: https://github.com/slb350/drep/compare/v2.3.0...v2.4.0
[2.3.0]: https://github.com/slb350/drep/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/slb350/drep/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/slb350/drep/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/slb350/drep/releases/tag/v2.0.0
