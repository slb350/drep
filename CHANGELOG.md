# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  existing four. Verified against the live endpoints on 2026-08-19 with a
  seven-line Python fixture: z.ai `glm-5.3` returned 7 findings in 32.6s,
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

- The shared Rust workflow now matches each forge's actual runner coverage.
  Linux tests run on both GitHub and the family Gitea instance, while the macOS
  job is GitHub-only. Gitea evaluates a job guard after runner matching, so the
  guarded skip uses its Linux label there instead of waiting forever for a
  nonexistent family macOS runner. The mutation job uses `node:22-trixie` and
  pins `cargo-mutants` 27.1.0
  because the runner's default Debian 12 image has glibc 2.36 and cannot execute
  that release's glibc 2.39 binary. The cache read-error regression test now
  creates an unreadable entry shape deterministically instead of relying on
  mode `000`, which root can still read inside the Gitea job container.
- `rust.yml` had never run: `rust-rewrite` only ever went to Gitea, so CI first
  executed when 2.0 merged to `main` - and three jobs failed. The MSRV was
  wrong: `ignore` 0.4.30 uses let-chains and declares no `rust-version` of its
  own, so cargo could not refuse the resolve and the failure arrived as a
  compile error inside a dependency. Measured, 1.86 and 1.87 fail and 1.88
  builds, so the declared floor is 1.88 rather than the 1.85 open-agent-sdk
  asks for. Pinning `ignore` back would have held the old floor at the cost of
  freezing the file walk. Raising it turned on four MSRV-gated clippy lints,
  since `is_multiple_of` and collapsing an `if` into a let-chain both need a
  compiler the old floor did not promise; `src/docs/blocks.rs`,
  `src/docs/links.rs`, `src/llm/json_parsing.rs` and one test now use them.
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
- The mutation sweep tested a tree the commit did not have.
  `scripts/mutants-remote.sh` synced with `--delete`, and an excluded name
  inside a directory protects that directory from removal - so
  `docs/api/build/html` kept `docs/api` alive on strix after the commit that
  deleted it, and `the_python_package_and_its_build_files_are_gone` failed
  there while passing here. `--force` does not cover it: it deletes non-empty
  directories, not protected ones. The sync now passes `--delete-excluded`
  with a `P /target` filter, so stale leftovers go and the 1.7GB build cache
  the offload exists to reuse stays.
- The v2.0.0 release built and published every binary, then failed to push the
  Homebrew formula. Not the token: the `publish-homebrew-formula` job passes the
  whole `dist plan` manifest to the runner as one environment variable, Linux
  caps a single environment variable at 128 KB (`MAX_ARG_STRLEN`), and the
  manifest was 190 KB - because cargo-dist embeds the released version's
  changelog section *twice*, and that section was 87 KB of phase-by-phase
  rewrite narrative. `execve` of node failed with E2BIG before `actions/checkout`
  ever authenticated. The narrative moved to `docs/rewrite-log.md` and the 2.0.0
  entry became release notes; the manifest is now 15 KB. The 2.0.0 formula was
  pushed to the tap by hand, so `brew install slb350/tap/drep` works.

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
- Configuration is `drep.toml`, not `config.yaml`. `api_key` names an
  environment variable rather than holding a secret.
- Python, JavaScript, TypeScript, Go and Rust are all first-class. Adding a
  language is an entry in one table.

### Removed

- The webhook server, the Gitea/GitHub/GitLab adapters, the issue manager, the
  PR reviewer, docstring generation, the SQLite database and the metrics
  dashboard. drep talks to no platform and stores nothing but a response cache.
- The Python package. `drep-ai` 1.3.0 stays on PyPI unyanked; nothing new is
  published there.

The phase-by-phase development narrative is in
[docs/rewrite-log.md](docs/rewrite-log.md).

## [1.3.0] - 2026-08-16

**The local gate.** drep now runs on `git push` against Python, JavaScript, TypeScript, Go
and Rust — using the linters and formatters your project already configures, with LLM
review alongside. It works with no configuration at all: the deterministic half needs no
model, no key and no tokens.

```bash
curl -fsSL https://raw.githubusercontent.com/slb350/drep/main/scripts/install.sh | bash
```

Two layers, split by **source** rather than severity — which is what makes the gate usable.
ruff and eslint are precise enough to block a push; an LLM's opinion about naming is not, at
any severity:

| Layer | Source | Blocks? |
|---|---|---|
| Deterministic | ruff, eslint, tsc, gofmt, go vet, clippy | **Yes** |
| Semantic | Your chosen LLM | No — reported only |

A tool runs only where the project configured it, and a configured-but-missing tool exits
**2** rather than reporting those files clean.

### Added - 2026-08-16
- **`scripts/install.sh`** — curl-able installer that adds drep as a pre-push gate to any
  repository. Deliberately thin: provider presets live in `drep init-llm` and detection in
  `drep doctor`, so the script never duplicates that knowledge. What is left is genuinely
  shell-shaped — finding a Python, installing the package, and working around
  `core.hooksPath`, where git ignores `.git/hooks` entirely and both failure modes are
  silent. Idempotent, and skips the model prompt when piped from curl with no tty.
- **`drep doctor`** — reports what drep will actually do here: languages present, which
  tools are ready, which are unconfigured, and which are configured but missing. Never
  fails; it is diagnosis, not a gate.
- **`drep init-llm --provider {local,openai,openrouter,custom}`** — writes just the `llm`
  block from a named preset, without `drep init`'s platform credentials that a local gate
  never uses. Cloud presets ship a reasoning-sized token budget, and write `${VAR}` rather
  than the key, since config.yaml is usually committed.
- **Multi-language analysis.** drep now analyzes Python, JavaScript, TypeScript, Go and
  Rust. Discovery, analyzer support and prompts all route through the language registry,
  so no `if python` branch exists anywhere.
  - **Deterministic findings gate; LLM findings inform.** `AnalysisResult.blocking` carries
    tool findings, `findings` carries the LLM's. `--fail-on` now *opts in* to gating on LLM
    findings rather than setting a threshold that always applied - the model reports style
    suggestions on nearly every file, so gating on them by default meant nothing passed.
  - **A configured-but-missing tool exits 2**, distinct from a file the LLM could not
    analyze: `AnalysisResult.unavailable_tools`. "eslint is missing" and "this file went
    unanalyzed" both mean the run was incomplete, but they need different words.
  - The docstring pass keeps its independent `is_python_source` filter, so `ast.parse` can
    never be handed a Go file. Pinned by test.
  - Tool paths are normalised to repo-relative: ruff reports absolute, go vet relative.
- **Language registry** (`drep/languages/`) — the foundation for multi-language support,
  with no `if python` branching anywhere. A `LanguageSupport` carries the extensions it
  owns, the deterministic tools that check it, and the conventions its LLM prompt names.
  Registered today: Python, JavaScript, TypeScript, Go, Rust.
- **Deterministic tool runner** (`drep/languages/runner.py`). Two-layer analysis: the
  project's own tools (ruff, eslint, tsc, gofmt, go vet, clippy) are precise, so their
  findings gate; the LLM's semantic findings inform. Splitting by *source* rather than by
  severity is what makes the gate calibratable — no `--fail-on` guessing.
  - Binaries resolve **repo-local first, then PATH**, so a project is checked by the
    version its own CI runs (`node_modules/.bin/eslint`, not whatever is global).
  - A tool runs only where the project **configured** it: no eslint config means no eslint
    findings, rather than a wall of default-preset complaints.
  - Three distinct outcomes: `ok`, `skipped` (the project didn't opt in — a pass), and
    `unavailable` (should have run, couldn't — **not** a pass). The last one is the same
    "unanalyzed is not clean" invariant that `drep check` now enforces.
  - Output parsers for ruff/eslint JSON, gofmt line output, compiler-style positions,
    tsc, and cargo's newline-delimited JSON. `go vet` writes diagnostics to **stderr**, so
    `ToolSpec.diagnostics_stream` selects the stream — reading stdout alone would have
    reported every Go file clean.
- **Local git hooks** (`.pre-commit-config.yaml`), split by cost:
  - **pre-commit** - ruff check, ruff format, `drep lint-docs`. Instant, deterministic.
  - **pre-push** - `drep check --fail-on error`. A reasoning model costs minutes and real
    money per file, which is fine once per push and intolerable on every commit.
  Both run from `./venv` via `language: system`. `lint-docs` is report-only here on
  purpose - its `long_line` check contradicts this repo's `MD013: false`.
- **`drep check` accepts multiple paths** (`nargs=-1`, defaults to `.`), rooting analysis at
  their common ancestor. This is what the pre-push hook passes: at push time nothing is
  staged, so `--staged` would analyze nothing and pass silently, while pre-commit resolves
  filenames to exactly the files the outgoing commits touch.
- **`max_tokens` ceiling raised to 200000 and `timeout` to 3600s.** Reasoning models bill
  `reasoning` against the completion budget and emit no content if they exhaust it, so the
  old 20000/300s ceilings made them unusable rather than merely slow.
- **`drep check --fail-on {info,warning,error}`** (default `info`, unchanged behaviour):
  the severity at or above which a finding blocks. Everything is still reported; only the
  exit code changes. Without it a commit gate is unusable - the LLM emits info-level style
  suggestions on almost every file, so exit 1 on any finding means no commit ever passes.
- **`drep lint-docs --strict`**: exits 1 when issues are found, so the rule-based markdown
  checks can gate a commit. Default behaviour is unchanged (report, exit 0).
- **`drep lint-docs` accepts multiple paths** (`nargs=-1`, defaults to `.`), which is what
  pre-commit passes.
- **`drep-lint-docs` hook id** in `.pre-commit-hooks.yaml` for downstream consumers.

### Changed - 2026-08-16
- **`drep check --format json` emits an object**, `{"findings": [...], "unanalyzed": [...]}`,
  and always emits it. A bare array looked identical whether every file was analyzed or
  none were. Safe to change: the array was unparseable until the status-line fix in this
  same release, so no working consumer existed.
- **`Finding.severity` is a validated `Severity` enum** with `SEVERITY_RANK` beside it.
  The ordering previously lived in `drep/cli.py` and defaulted unknown values to `info`,
  so a producer emitting the PR-review vocabulary (`critical`) could never block a gate.
- **`_run_check` returns `AnalysisResult`**; the near-identical `CheckOutcome` is gone.
- **Scanner passes return `AnalysisResult(findings, failed_files)`** instead of a bare
  finding list. `analyze_code_quality` / `analyze_docstrings` now name the files they could
  not analyze, rather than leaving the caller to infer it from a progress callback.
  `drep scan` warns when a scan was incomplete instead of printing only "Found N issues".
- **Shared pruning walk** — `file_targets.walk_targets()` is the one directory traversal;
  `RepositoryScanner.get_scan_targets` and `lint-docs` both use it.
- **`CodeQualityAnalyzer.analyze_files` deprecated** (no production callers; removal in
  1.4.0), matching the existing `ParallelAnalyzer` deprecation.

### Fixed - 2026-08-16
- **The hooks published for other repos did not work.** `language: system` meant
  pre-commit never installed drep, so the hook failed unless the consumer had already
  installed `drep-ai` themselves; `types: [python]` meant they never fired in a Go or
  TypeScript repo; and there was no pre-push variant, only a `--staged` one that analyzes
  nothing at push time. Now `language: python`, `types_or` across every registered
  language, and a `drep-check-push` hook.
- **`IGNORED_DIRS` predated multi-language support** — no `node_modules`, `target` or
  `vendor`, so adding JS, Rust and Go coverage meant walking into dependency trees and
  reporting findings against code the project does not own.
- **The pre-push hook still passed `--fail-on error`**, which opted the LLM back into
  gating and blocked a real push with 18 "blocking" findings that were model opinions,
  not tool output — reintroducing the exact problem the two-layer split solved. The hook
  now relies on deterministic findings to gate.
- **Advisory output crashed pre-commit.** 122 advisory findings printed with their full
  multi-line suggestions overwhelmed its writer (`BlockingIOError: [Errno 35]`). Blocking
  findings still print in full; advisory ones print one line each, with the full text
  still available via `--format json`.
- **`llm: {enabled: false}` was rejected by config validation** - the exact config the
  README documents for disabling LLM features. The provider validators checked the
  provider but never `enabled`, so switching the LLM off still demanded an endpoint and
  model for a backend that would never be contacted.
- **`--format json` summary prose corrupted stdout.** In JSON mode the human summary now
  goes to stderr, so the payload parses on its own.
- **Text output dropped the rule code.** `[F401]` is the actionable half of a
  deterministic finding - without it you cannot look the rule up or suppress it.
- **An incomplete `drep scan` recorded its SHA as scanned.** The next run diffs against
  that SHA, so every file the LLM never saw was excluded from all future incremental scans
  until it changed again - the same "unanalyzed is not clean" mistake, one layer down.
  A scan with unanalyzed files is no longer recorded.
- **`drep check a.py .` analyzed `a.py` twice.** Path expansion is now a single deduping
  `file_targets.expand_paths()` shared with `lint-docs`; a duplicate costs a whole extra
  LLM round-trip, not just a repeated report line.
- **An empty response was retried three times.** `finish_reason='length'` is deterministic,
  so retrying spent a full reasoning generation per attempt to reach the same answer.
  `EmptyLLMResponseError` now fails fast alongside `CircuitBreakerOpenError`.
- **The empty-content guard missed Bedrock**, which returns `""` rather than null; the
  check now sits after the transport split so every provider gets it.
- **The cache accepted poisoned entries on write**, not just filtered them on read.
- **Badge syntax `[![alt](img)](href)` was flagged as broken markdown** - the link pattern
  stopped at the image's own `]`. 11 false positives on this repo's own README.
- **`drep lint-docs` paid 119ms of unused import** on every commit (190ms → 71ms): `cli.py`
  imported `cli_workflows` at module scope, pulling in sqlalchemy, GitPython and the LLM
  client for a command that touches none of them.
- **Docstring generation awaited one function at a time**, so a file with N undocumented
  functions cost the sum of N round-trips; they are now gathered under the rate limiter.
- **`drep check` reported unanalyzed files as clean.** `CodeQualityAnalyzer.analyze_file`
  and `DocstringGenerator._generate_docstring` swallowed every exception and returned
  empty, so an unreachable LLM endpoint printed "✓ No issues found" and exited 0 — a
  pre-commit hook that rubber-stamped every commit. Both now propagate; `RepositoryScanner`
  counts the file as failed; `_run_check` returns `CheckOutcome(findings, unanalyzed)`; and
  `drep check` exits **2** with "N file(s) could not be analyzed".
- **`max_retries: 0` never sent the request.** `range(self.max_retries)` skipped the loop
  entirely and raised `RuntimeError("LLM request failed but no exception was captured")`,
  hiding the real transport error. Attempts now floor at 1, so failures name their cause.
- **`lint-docs` walked ignored directories**, reporting on `venv/`, `build/`, and
  `*.egg-info`. It now uses the shared pruning walk (126ms → 1.6ms here, and 38 real
  files instead of 62).
- **Markdown check false positives** — 1668 findings across this repo became 256:
  - `missing_space_after_heading` fired on *every* well-formed heading below level 1:
    `^#{1,6}\S` backtracks to one `#` and matches the second one.
  - Heading checks ignored code fences, flagging `#!/bin/bash` in every bash sample.
  - `link_syntax_invalid` counted parentheses line-locally, flagging any prose
    parenthetical that wrapped onto a second line.
  - `bare_url` flagged URLs inside inline-code spans.
  - `bare_url` *missed* a real bare URL whenever any well-formed link appeared on the same
    line. Well-formed links are now blanked and the remainder searched, rather than the
    whole line being excused (this is why the repo baseline is 269, not 256).
  - Dead `else 1` branch in the `empty_heading` level calculation - `_HEADING_EMPTY` only
    matches a run of `#`, so `split()` is never empty.
- **`drep check --format json` emitted a status line on stdout**, so "JSON output for tools"
  did not parse. `Checking N file(s)...` now goes to stderr.
- **A `content: null` response crashed four frames deep** as "argument of type 'NoneType'
  is not a container or iterable", from inside the JSON parser. Reasoning models that spend
  their whole budget on `reasoning` return exactly that. The client now raises a named
  error quoting `finish_reason`, so `length` points straight at `max_tokens`.
- **The poisoned entry was cached.** The bad response was written to the cache *before* the
  parser choked, so every later run replayed `content=None` in 0.6s without ever calling
  the LLM again. A cached entry with empty content is now treated as a miss.
- **`is_ignored_dir` was case-sensitive** while the module promised "identical,
  case-insensitive decisions" - a directory named `VENV` or `.Git` was descended into and
  analyzed. It now casefolds, matching the suffix predicates.
- **A response without a `usage` block crashed the SDK path.** `usage` is optional in the
  OpenAI schema and some local servers omit it; the HTTP fallback already defaulted it,
  the open-agent-sdk path did not. Since analyzer failures now propagate, that latent
  `AttributeError` would have marked the file unanalyzed rather than being swallowed.

### Still planned
- Vector database integration for cross-file context
- Custom rule definitions
- Metrics dashboard
- Notification system (Slack, Discord)
- Multi-repository analysis features

## [1.2.0] - 2026-08-16

Audit-driven correctness and simplification release. All 24 accepted findings from the
simplification audit resolved; rejected findings (C9, C11) documented with rationale.
A second `/simplify` pass over the merged branch (reuse, simplification, efficiency,
altitude) resolved a further round of findings — recorded below under their headings.

### Fixed - Reliability
- **Rate-limit permit leak on cancelled entry** (C3): cancellation or a rate-check error
  during `RateLimitContext.__aenter__` leaked the already-acquired global/repo semaphore
  permits permanently (a leaked repo permit blocked that repo forever). Permits are now
  rolled back on any `BaseException` mid-entry.
- **Unobserved background-task failures** (C19): failed webhook scans surfaced only as
  asyncio warnings; the done-callback now retrieves and logs exceptions with context.
- **Env substitution re-typed config values** (C16): the yaml.dump → regex → reload
  round-trip re-parsed env values as YAML (`true` became bool, `123` became int,
  `a: b` corrupted structure). Substitution now runs over the parsed tree, strings only.
- **Aggregate metrics lost latency and analyzer data** (C6): `to_dict` persists raw
  `total_latency_ms`; a model-owned `merge_serialized` owns the full field set (latency
  total, `by_analyzer`) with legacy backfill. `drep metrics --days` reports real averages.
- **Nested-function detection missed control-flow nesting** (C12): helpers defined under
  `if`/`try`/`with` inside a function leaked in as docstring candidates.
- **Webhook payload shape** (C18): non-object JSON bodies now get a 400 instead of a 500.

### Fixed - Correctness of the review pipeline
- **Review results bound to their diff** (C13): `review_pr` returns an immutable
  `PreparedReview` (result, anchor, per-file added lines); `post_review` consumes it.
  Previously a second review silently re-anchored the first review's line validation.
- **One anchor fetch per review** (C2): new `BaseAdapter.get_review_anchor()`; GitLab
  validates and carries MR `diff_refs` (base/head/start SHAs) once per review instead of
  refetching the MR for every inline comment (n comments = n MR fetches, with mid-loop
  version drift).
- **GitHub adapter duplication** (C1): the SHA-explicit inline-comment method is now
  canonical and owns the 422 invalid-line handling; the one-shot variant resolves the
  anchor and delegates.

### Changed
- **File-size discipline**: every Python file is now under the 800-line limit.
  `drep/llm/client.py` split into `rate_limiter`/`json_parsing`/`git_utils` (public names
  re-exported), `drep/cli.py` into `cli_wizard`/`cli_workflows`, adapter review/PR methods
  into mixin modules (`gitlab_prs`, `gitlab_reviews`, `github_reviews`), and the largest
  test files split by topic. No public names removed.
- **File-target policy unified** (C7): one case-insensitive suffix policy across full
  scans, commit diffs, staged files, and per-analyzer filters. `TEST.PY` is no longer
  silently never analyzed.
- **Circuit breaker is real** (C5+C23): HALF_OPEN is an exclusive single-probe state
  (dead `half_open_max_calls` removed); both provider transports run through the
  breaker, which previously was constructed but never invoked. Open circuits fail fast
  instead of being retried.
- **Version single-sourced** (C24): `drep.__version__` is the only version declaration
  (pyproject reads it dynamically; FastAPI app imports it). Was drifting across four
  places.
- **Platform selection single-sourced** (C17): duplicated gitea→github→gitlab chains in
  the scan and review workflows collapsed into `_resolve_platform`.
- **CI selects tests by marker** (C21): new `external_service` marker on the truly-live
  test modules; CI runs `-m "not external_service"` instead of name-based exclusion,
  restoring ~43 silently-excluded hermetic tests to CI (839 vs 796).
- **`issue_number` is NOT NULL** (C20): a NULL row suppressed a finding forever; legacy
  SQLite tables are migrated (NULL rows dropped so findings can be re-reported).

### Fixed - Concurrency and rate limiting (second pass)
- **Rate limiter held its lock across sleeps**: both `_check_request_rate_limit` and
  `_check_token_rate_limit` slept while holding the shared lock, which blocked
  `RateLimitContext.__aexit__` — needing the same lock to reconcile tokens before
  releasing permits — and stalled all LLM traffic for the duration of the wait.
- **Unsatisfiable token reservation spun forever**: a request estimating more tokens
  than the entire per-minute budget could never satisfy the bucket condition and looped
  indefinitely (previously while holding the lock, deadlocking every request).
  `RateLimiter.request()` now clamps the reservation.
- **Concurrency permits taken before rate waits**: a merely time-throttled request
  occupied one of the scarce `max_concurrent` slots while sleeping, dropping effective
  concurrency well below the configured value. The requests-per-minute wait now happens
  first.
- **Circuit-breaker probe reservation could be cleared by another task**: `finally`
  cleared `_probe_in_flight` unconditionally, so a CLOSED-state call finishing after the
  breaker re-entered HALF_OPEN wiped a probe reservation and let probes dogpile the
  recovering service. Only the reserving call clears it.
- **Webhook config failures no longer fail open**: `_load_webhook_secret` caught every
  exception and returned `None`, downgrading the endpoint to accepting unauthenticated
  requests on a YAML typo. A missing config still means "unset"; an unreadable one is
  now rejected with 503.
- **`ValidationError` escaped the JSON fallback ladder**: a response that parsed but
  failed schema validation propagated out of `extract_json`, past the stricter-prompt
  retry in `analyze_code_json` that exists for exactly that case. All strategies now
  share one recoverable-exception policy (and no longer swallow real bugs via bare
  `except Exception`).
- **Gitea inline-comment fallback retried on any status**: the `new_position` →
  `position` retry exists for older Gitea versions rejecting the field name (400/422),
  but fired on 401/403/404/5xx too, issuing a pointless second POST and burying the real
  cause in a compound error message.

### Changed - Performance (second pass)
- **Per-file LLM analysis runs concurrently**: `analyze_code_quality` and
  `analyze_docstrings` awaited one round trip at a time, so a scan took the *sum* of all
  LLM latencies while the configured `max_concurrent_global`/`max_concurrent_per_repo`
  went unused. Both now gather; the rate limiter supplies back-pressure.
- **Review fetches the PR once**: `get_review_anchor` internally called `get_pr`, then
  the prompt fetched the identical payload again. Anchor derivation is now the pure
  `anchor_from_pr(pr_data, ...)` hook, and the PR and diff are fetched concurrently.
  The diff is also no longer re-serialized from parsed hunks for the prompt.
- **Per-repo rate limiting reached PR reviews**: `repo_id` was computed and threaded
  through `_analyze_diff_with_llm` but never passed to the LLM client, silently
  disabling per-repo limiting for reviews. It and `commit_sha` (cache key) now are.
- **Scan discovery prunes ignored trees**: `rglob("*")` stat'd every entry under `.git`,
  `venv`, `build` before filtering; discovery now walks with in-place directory pruning.
- **Cheaper hot paths**: idle repo-semaphore eviction sweeps once per TTL instead of
  scanning all tracked repos per request; `request_times` is a deque; the webhook secret
  is cached per (path, mtime) instead of re-parsing the config per request; the LLM cache
  tracks a running byte total instead of stat'ing the whole cache directory on every
  write; the metrics history file is capped and written off the event loop;
  `git rev-parse` runs via `asyncio.to_thread`; `fuzzy_inference` regexes are compiled
  once per (schema, field); the issue-manager dedup check is one query instead of one
  per finding; `_collect_function_nodes` no longer walks function bodies to build lists
  it always discarded.

### Changed - Structure (second pass)
- **Adapter contract shrank to what actually varies**: `post_review_comment` and
  `close()` are concrete on `BaseAdapter` (three byte-identical overrides removed;
  Gitea gains close()'s error handling). A shared `_network_error()` replaces 15 copies
  of the timeout/connect cascade, and `_decode_file_content()` the duplicated base64
  decode. `git_clone_url()` is a new adapter method, so GitHub Enterprise and GitLab
  self-hosted URL rules leave the shared workflow.
- **Mixins inherit instead of restating their host**: `GitHubReviewMixin`,
  `GitLabPrMixin` and `GitLabReviewMixin` derive from `BaseAdapter`/`GitLabMixinBase`
  rather than each re-declaring the host surface in a duplicate `if TYPE_CHECKING` block.
- **Platform identity derives from the typed data**: each `*PlatformData` variant carries
  `config_key`/`display_name`/`token_env_var`, removing four scattered mappings —
  including a `"${LLM_API_KEY}" in yaml.dump(config_dict)` string search in the CLI for a
  fact the model already held, and an isinstance chain whose fallthrough silently
  labelled unknown variants "gitlab".
- **New shared modules**: `drep/core/file_targets.py` (one file-target policy the
  analyzers reuse instead of re-implementing suffix checks), `drep/logging_utils.py`
  (one secret-redaction helper, previously inlined in three places),
  `drep/llm/http_compat.py` (the OpenAI-shaped HTTP shim, previously six classes rebuilt
  inside every `LLMClient.__init__` and closing over the whole init frame).
- **`load_raw_config()`** in `drep/config.py` is shared by `load_config` and the webhook
  server, which had hand-rolled the same read/substitute sequence and reached across
  modules for a private helper.
- **`_run_check` moved to `drep/cli_workflows.py`** beside `_run_scan`/`_run_review`, and
  returns findings instead of printing them.
- **The `finding_cache` migration builds its table from the model** rather than a raw DDL
  string that could silently drop a future column, and probes on a read-only connection.
- **`RepositoryScanner._get_all_python_files` → `get_scan_targets`** (it returns `.md`
  too), and `PRReviewAnalyzer`/`RepositoryScanner` take `adapter`, not `gitea_adapter`.

### Breaking Changes (second pass)
- **`PlatformConfig.env_var` is a derived property**, no longer a constructor argument.
- **`BaseAdapter.git_clone_url(owner, repo)` is abstract** — custom adapters must
  implement it. `post_review_comment` and `close()` are no longer abstract.
- **`PRReviewAnalyzer(llm_client, adapter)`** — the second parameter was `gitea_adapter`.
- **`RepositoryScanner(db, config, adapter=...)`** — the keyword was `gitea_adapter`.

### Security
- **Webhook HMAC verification** (out-of-audit finding): optional `webhook_secret`
  config field enables constant-time HMAC-SHA256 verification of `X-Gitea-Signature`
  over the raw body; requests with missing/invalid signatures are rejected with 403.
  Unset secret keeps existing deployments working (warning logged).

### Removed
- **`drep/security/` package** (C10): `detect_secrets_in_logs`/`sanitize_url` had zero
  production callers; safe-logging practices live inline in the adapters.
- **Six TODO-stub modules** (S15): `core/llm_agent`, `core/code_analyzer`,
  `db/repository`, `models/webhook`, `documentation/llm_review`,
  `documentation/comment_gen`.
- **Personal endpoint hardcoded in `scripts/test_llm_client.py`** (C25): env-driven
  (`DREP_TEST_LLM_ENDPOINT`) with a neutral localhost default.

### Deprecated
- `ParallelAnalyzer` and `timeout_with_partial_results` (C8): zero production callers;
  emit `DeprecationWarning`, removal planned for 1.3.0.

### Breaking Changes
- **`llm.provider` is a closed set: `openai-compatible` | `bedrock`** (C14). The wizard
  no longer offers `anthropic` (it generated configs guaranteed to fail at runtime).
  Migration: use `provider: openai-compatible` with an OpenAI-compatible Anthropic
  proxy endpoint, or `provider: bedrock` with Claude model IDs.
- **Wizard wrappers are typed-only** (C15): `PlatformConfig`/`LLMConfig`/
  `DocumentationConfig` no longer accept the deprecated raw-dict `config=` field;
  platform key, display name, and provider derive from the typed data.
- **`create_pr_review_comment(anchor, file_path, line, body)`** (C2/C13): takes an
  immutable `ReviewAnchor` from `get_review_anchor()` instead of
  `(owner, repo, pr_number, commit_sha)`. GitLab requires a `GitLabReviewAnchor`.
- **`PRReviewAnalyzer.post_review(prepared)`** (C13): takes the `PreparedReview`
  returned by `review_pr`.
- **`FindingCache.issue_number` is NOT NULL** (C20).

## [1.1.3] - 2026-08-16

### Changed - Tooling & Code Modernization 🧹

**Maintenance:** Internal quality improvements only — no user-facing behavior changes.

- **Ruff expanded from 3 rule groups to 16** (added pyupgrade, bugbear, simplify,
  comprehensions, return, pie, perflint, use-pathlib, refurb, ruf, pylint C/E) and
  the entire codebase brought into compliance (~450 issues resolved, including all
  101 `raise`-without-`from` exception-chaining violations).
- **Black replaced by `ruff format`** — one tool for lint + format; `black` removed
  from dev dependencies.
- **CI modernized**: lint/format/type checks split into a fast `lint` job; test
  matrix now covers Python 3.10–3.14 (previously 3.13 only); actions bumped
  (`checkout@v5`, `setup-python@v6`).
- **Refactors**: deduplicated function/method AST extraction in
  `drep/docstring/ast_utils.py`; hoisted stray function-level imports (documented
  lazy imports kept with `noqa`); `open()` calls migrated to `pathlib`.
- **Dependencies refreshed** across the board (`uv.lock` regenerated).

### Fixed
- **Fire-and-forget webhook tasks could be garbage-collected mid-run**:
  `drep/server.py` now holds strong references to background scan/review tasks
  until they complete (RUF006).

## [1.1.2] - 2026-05-24

### Changed - Type-Safety Hardening 🔍

**Maintenance Release:** Internal type-safety improvements only — no user-facing
behavior changes.

- **mypy is now clean across `drep/`** (resolved all 62 pre-existing errors across
  11 modules) and runs as a CI gate, so type regressions are caught automatically.
- **`BaseAdapter` contract completed**: added `create_pr_review_comment`, which all
  three adapters already implemented and the PR-review workflow depends on. The
  `PRReviewAnalyzer`/`RepositoryScanner` adapter parameters were widened from
  `GiteaAdapter` to `BaseAdapter`, so `drep review` against GitHub/GitLab is now
  correctly typed.
- **Modernized SQLAlchemy models** to use the 2.0 `DeclarativeBase`.

### Fixed
- **Incorrect exception reference** in the GitHub and GitLab adapters: caught
  `base64.binascii.Error` (which only resolved by accident) is now the correct
  `binascii.Error`.

### Development
- Added `[tool.mypy]` configuration (pydantic plugin; `ignore_missing_imports` for
  boto3/botocore) and a `mypy drep` step to the CI workflow.

## [1.1.1] - 2026-05-24

### Fixed

- **Gitea inline review comments rejected with "review event requires a body"** (#11):
  `GiteaAdapter.create_pr_review_comment()` submitted the pull review with an
  empty top-level `body`. Some Gitea versions enforce a non-empty body even when
  inline comments are present, so every inline comment request was rejected and
  no review feedback was posted.
  - Both the `new_position` and `position` payloads now send a short generic
    placeholder (`REVIEW_BODY_PLACEHOLDER`) as the review body.
  - The actual finding still appears only in the inline comment, so it is not
    duplicated as a review summary (preserving the original empty-body intent).

### Testing
- **1 new test added** (796 total passing, excluding integration)
- `test_create_pr_review_comment_always_sends_non_empty_body`: regression test
  asserting a non-empty body is sent even when the inline comment text is empty
- Updated `test_create_pr_review_comment_sends_correct_payload` to assert the
  placeholder body

### Development
- Fix contributed via PR #12 by Rain Wu (@facewhy); refined to use an
  unconditional placeholder rather than echoing the finding text.

## [1.1.0] - 2025-11-09

### Added - Interactive Configuration Wizard 🧙‍♂️

**Feature Release:** Comprehensive `drep init` wizard for guided configuration setup!

- **Interactive CLI Wizard**: Step-by-step configuration generator
  - Platform selection (Gitea, GitHub, GitLab)
  - Enterprise server detection (GitHub Enterprise, self-hosted GitLab/Gitea)
  - Repository pattern configuration with wildcard support
  - LLM provider selection (OpenAI-compatible, AWS Bedrock, Anthropic)
  - Documentation analysis settings with markdown linting options
  - Custom database URL configuration
  - Advanced LLM settings (temperature, max tokens, rate limits)
  - Environment variable verification and guidance

- **Config Discovery**: Flexible configuration file locations
  - Current directory: `./config.yaml` (project-specific)
  - User config directory: `~/Library/Application Support/drep/config.yaml` (system-wide)
  - User selects preferred location during wizard

- **Input Validation**: Real-time validation with helpful error messages
  - `URLType`: HTTP/HTTPS URL validation with scheme checking
  - `RepositoryListType`: Repository pattern validation (`owner/repo`, `owner/*`)
  - `BedrockModelType`: AWS Bedrock model ID validation (anthropic.*, amazon.*, etc.)
  - `DatabaseURLType`: SQLAlchemy database URL validation
  - `NonEmptyString`: Required field validation
  - Duplicate repository pattern detection and deduplication

- **Strongly-Typed Wizard Models**: 7 new frozen dataclasses
  - `GitHubPlatformData`: GitHub platform configuration with optional enterprise URL
  - `GiteaPlatformData`: Gitea platform configuration
  - `GitLabPlatformData`: GitLab platform configuration
  - `OpenAILLMData`: OpenAI-compatible LLM configuration
  - `BedrockLLMData`: AWS Bedrock LLM configuration with region/model
  - `BedrockRegionModel`: Nested Bedrock region and model settings
  - `AnthropicLLMData`: Anthropic API configuration
  - `DocumentationConfigData`: Documentation analysis settings
  - All models use tuples for immutability (not lists)
  - `to_dict()` methods convert tuples → lists for YAML serialization

### Testing - Security & Integration
- **13 new tests added** (795 total tests passing)

**Finally Block Error Handling (3 tests)**:
- Test cleanup errors don't mask scan errors (ValueError vs OSError)
- Test successful cleanup allows scan error propagation
- Test cleanup failure is silent when main operation succeeds
- **Finding**: All finally blocks already correct - verification tests added

**Token Leakage Prevention (5 tests)**:
- Test GitHub token never logged (caplog + stdout + config file)
- Test Gitea token never logged
- Test Anthropic API key never logged
- Test environment variable checks mask token values
- Test multiple tokens all masked simultaneously
- **Security**: Wizard uses placeholders (`${GITHUB_TOKEN}`) not actual values

**End-to-End Integration (5 tests)**:
- Test GitHub end-to-end (wizard → load_config → adapter creation)
- Test Gitea + Bedrock end-to-end (complex nested config)
- Test GitLab + Anthropic end-to-end
- Test custom database URL configuration
- Test malformed YAML caught gracefully by validator
- **Integration**: Verifies complete workflow from wizard → scan

### Changed
- **CLI Workflow**: `drep init` now generates fully validated configurations
- **User Experience**: Guided setup replaces manual YAML editing
- **Error Prevention**: Input validation catches issues before config file creation
- **Security**: Environment variable placeholders prevent token leakage

### Improved
- **Validation**: Click validators provide immediate feedback during input
- **Documentation**: Inline help text and examples throughout wizard
- **Defaults**: Sensible defaults for all optional settings (temperature=0.2, max_tokens=4000)
- **Error Messages**: Clear, actionable messages for validation failures

### Development
- **Zero Tech Debt Policy**: All 3 critical PR review issues resolved
- **TDD Methodology**: All 13 tests written first, then features implemented
- **Type Safety**: Strongly-typed wizard models prevent runtime errors
- **Immutability**: Frozen dataclasses with tuple-based collections

## [1.0.0] - 2025-11-09

### Added - GitLab Platform Support (Phase 3.5) 🎉

**Production Release:** drep now supports all three major git platforms!

- **GitLab Adapter**: Complete GitLab REST API v4 implementation
  - Full BaseAdapter compliance (all 8 abstract methods)
  - Support for both GitLab.com and self-hosted instances
  - URL-encoded project paths (owner%2Frepo)
  - PRIVATE-TOKEN authentication header
  - Merge request (MR) reviews with discussion API
  - Position objects for inline comments
  - Base64-encoded file content support
  - Diff reconstruction from JSON array format

- **Platform Coverage**: Production-ready support for:
  - ✅ Gitea (self-hosted and Gitea.com)
  - ✅ GitHub (GitHub.com and GitHub Enterprise)
  - ✅ GitLab (GitLab.com and self-hosted instances)

- **API Compatibility Fixes**:
  - Normalized `get_pr()` response to include `head.sha` field
  - Added `create_pr_review_comment()` method for PRReviewAnalyzer compatibility
  - Consistent API across all three platform adapters

### Testing
- **93 GitLab adapter tests** (up from 35 after fixes)
  - Comprehensive JSON validation tests
  - Network error handling (timeout, connection failures)
  - HTTP error code tests (401, 403, 500, 503)
  - Rate limit edge cases with parametrized tests
  - URL handling tests (/api/v4 suffix deduplication)
- **618 total tests passing** - All platforms verified
- **Test coverage**: 0.082 test/line ratio (71% above GitHub adapter)

### Changed
- **Production Status**: Development Status classifier updated to "5 - Production/Stable"
- **Platform Parity**: All three adapters (Gitea, GitHub, GitLab) feature-complete
- **CLI Integration**: `drep scan` and `drep review` commands support all platforms
- **Documentation**: Updated all docs to reflect GitLab support

### Improved
- **Error Handling**: GitLab adapter has superior error handling vs existing adapters
  - Consistent JSON validation across all endpoints
  - Comprehensive network error detection
  - Clear, actionable error messages with context
  - Proper rate limit detection and reporting

### Fixed
- **Codex Bot Issues** (PR #8):
  - Fixed missing `head.sha` field in GitLab MR responses
  - Fixed missing `create_pr_review_comment()` method
  - Both issues resolved for CLI compatibility

### Development
- **Zero Tech Debt Policy**: All critical issues resolved before release
- **Comprehensive Reviews**: Multi-agent code review process
- **TDD Methodology**: All features developed test-first
- **157% Test Increase**: From 35 to 93 tests during development

## [0.9.0] - 2025-11-08

### Added - Pre-Commit Hook Support (Phase 3.6)
- **New `drep check` command**: Local-only analysis without platform API requirements
  - `--staged` flag: Check only git staged files (pre-commit workflow)
  - `--exit-zero` flag: Warning mode without blocking commits
  - `--format` option: Output as `text` (default) or `json`
  - Works without Gitea/GitHub/GitLab tokens (local-only mode)
  - Respects LLM config when present for intelligent analysis
  - Pre-commit friendly output format (`file:line:column: severity: message`)

- **Pre-commit Integration**: `.pre-commit-hooks.yaml` in repository
  - `drep-check` hook: Checks staged files only
  - `drep-check-all` hook: Checks all Python files
  - Direct repo reference: `repo: https://github.com/slb350/drep`
  - Installation: `brew tap slb350/drep && brew install drep-ai` or `pip install drep-ai`

- **Staged File Detection**: `RepositoryScanner.get_staged_files()` method
  - Returns only Python (.py) and Markdown (.md) files
  - Handles new files, deleted files, and renamed files correctly
  - Designed specifically for pre-commit workflow

### Changed
- **Config Validation**: Platform config now optional for local-only mode
  - `load_config()` accepts `require_platform=False` parameter
  - Enables LLM-only configurations without Gitea/GitHub/GitLab
  - `Config.require_platform_config` field controls validation
  - Backward compatible (default behavior unchanged)

- **Exit Codes**: `drep check` returns exit code 1 when issues found
  - Properly blocks commits in pre-commit hooks
  - Use `--exit-zero` for warning-only mode

### Testing
- **12 New Tests**: Comprehensive TDD coverage
  - 6 tests for `get_staged_files()` method
  - 4 tests for optional platform config
  - 4 tests for `drep check` command
  - All 521+ tests passing

### Documentation
- `.pre-commit-hooks.yaml`: Pre-commit hook definitions
- Pre-commit integration ready (detailed docs in README to follow)

### Development Methodology
- **Strict TDD**: All features developed with Test-Driven Development
  - RED: Write failing tests first
  - GREEN: Implement to pass tests
  - REFACTOR: Improve code quality
  - COMMIT: Commit each TDD cycle

## [0.8.2] - 2025-11-08

### Added
- **Interactive Platform Selection**: `drep init` now prompts for platform choice
  - Interactive prompt with GitHub, Gitea, GitLab options
  - Default to GitHub (most common use case)
  - Platform-specific config templates generated automatically
  - Correct environment variable names per platform (GITHUB_TOKEN, GITEA_TOKEN, GITLAB_TOKEN)

### Improved
- **README Documentation**: Comprehensive setup guide with step-by-step instructions
  - Clear platform selection guidance
  - Detailed API token creation instructions for each platform
  - LLM backend setup options (LM Studio, Ollama, AWS Bedrock)
  - Reduced user confusion during initial setup
- **User Guidance**: Better error messages and next steps after `drep init`

### Changed
- `drep init` command behavior: Now interactive instead of generating Gitea-only config
- Default platform: GitHub (changed from Gitea)

### Fixed
- User confusion when trying to scan GitHub repositories with default Gitea config
- Missing platform-specific setup instructions

## [0.8.0] - 2025-11-08

### Added - AWS Bedrock Provider Support (Phase 3.3)
- **AWS Bedrock LLM Provider**: Full support for Claude models via AWS Bedrock
  - BedrockClient implementation with OpenAI-compatible interface
  - Support for Claude Sonnet 4.5 and Haiku 4.5 models
  - Automatic AWS credential chain authentication
  - Region-specific model deployment
  - Comprehensive error handling for AWS-specific errors
- **Configuration Enhancements**:
  - `BedrockConfig` for AWS region and model selection
  - Optional `endpoint` and `model` fields for Bedrock provider
  - Provider-specific validation (`openai-compatible` vs `bedrock`)
  - Support for `provider="bedrock"` in LLMConfig
- **Test Coverage**: 511 total tests (19 new Bedrock-specific tests)
  - Unit tests for BedrockClient (17 tests)
  - Integration tests for LLMClient with Bedrock (4 tests)
  - Configuration validation tests (3 tests)

### Fixed - Critical P1 Issues
- **Cache Corruption Fix** (P1): Preserve Bedrock model name in `LLMClient.model`
  - Previously: Different Bedrock models shared cache entries (model=None)
  - Impact: Model A could serve stale responses from Model B
  - Fix: Explicitly set `self.model = bedrock_model` during initialization
  - Result: Each model has distinct cache keys, metrics show actual model names
- **Async Event Loop Blocking** (P1): Wrap boto3 calls in `asyncio.to_thread()`
  - Previously: Synchronous `boto3.invoke_model()` blocked event loop
  - Impact: Defeated async concurrency, stalled rate limiting/progress tracking
  - Fix: Use `asyncio.to_thread()` to run boto3 in thread pool
  - Result: Event loop remains responsive, concurrent requests work properly
- **AWS API Compliance** (P1): Add required headers and encode body as bytes
  - Previously: Missing `contentType` and `accept` headers, body as string
  - Impact: Violates AWS Bedrock API spec, could cause ValidationError
  - Fix: Add `contentType="application/json"`, `accept="application/json"`, encode body as bytes
  - Result: Full AWS API compliance per boto3 documentation
- **Config Validation** (P1): Make `endpoint` and `model` optional for Bedrock
  - Previously: Required dummy values for Bedrock configs
  - Impact: Made feature unusable as documented
  - Fix: Optional fields with provider-specific validation
  - Result: Bedrock works without dummy endpoint/model values
- **Endpoint Handling** (P1): Handle `endpoint=None` gracefully
  - Previously: `endpoint.rstrip("/")` crashed with AttributeError on None
  - Impact: Blocked Bedrock initialization
  - Fix: Check if endpoint exists before calling methods
  - Result: Bedrock provider initializes with endpoint=None

### Fixed - Non-Blocking Issues
- **StreamingBody Resource Management**: Added explicit `close()` calls
  - Ensures proper cleanup of AWS response streams
  - Prevents resource leaks in long-running processes
- **Error Message Clarity**: Enhanced user-friendly AWS error messages
  - ThrottlingException, AccessDeniedException, ValidationException
  - Actionable guidance for common Bedrock errors
- **Code Quality**: Addressed all PR review feedback
  - Removed redundant exception handlers
  - Added explanatory comments for complex logic
  - Improved test coverage for edge cases

### Changed
- **Documentation Updates**:
  - README: Added AWS Bedrock setup instructions and configuration examples
  - Technical Design: Updated with Bedrock architecture details
  - LLM Setup Guide: Comprehensive Bedrock configuration walkthrough
  - Roadmap: Marked Phase 3.3 complete, added Phase 3.4 (Anthropic Direct)
- **Dependencies**:
  - Added `boto3` for AWS Bedrock support
  - Added `botocore` for AWS SDK functionality

### Development
- **TDD Methodology**: All fixes implemented with strict Test-Driven Development
  - RED phase: Write failing tests first
  - GREEN phase: Implement fixes
  - REFACTOR phase: Improve code quality
  - VERIFY phase: Run full test suite
- **Code Quality**: All ruff/black checks passing
- **Zero Technical Debt**: All P1 and non-blocking issues resolved

## [0.1.0] - 2025-10-19

### Added
- Initial release of drep (PyPI package: drep-ai)
- Platform adapters for Gitea, GitHub, and GitLab
- Three-tiered documentation analysis:
  - Layer 1: Dictionary spellcheck
  - Layer 2: Pattern matching for common issues
  - Layer 3: LLM-based analysis for complex cases
- Code analyzer with AST parsing and LLM-based detection
- Documentation specialist features:
  - Typo detection and correction
  - Grammar and syntax checking
  - Missing comment detection and generation
  - Bad comment identification and improvement
- Automated draft PR creation for documentation fixes
- Issue creation for code quality problems
- FastAPI webhook server for receiving platform events
- Background worker for asynchronous job processing
- SQLite database for finding cache and deduplication
- Click-based CLI with commands:
  - `drep init` - Initialize configuration
  - `drep serve` - Start webhook server
  - `drep scan` - Manual repository scan
  - `drep validate` - Validate configuration
- Configuration via YAML file with environment variable support
- Docker support with docker-compose example
- Support for multiple LLM backends via open-agent-sdk:
  - Ollama
  - llama.cpp
  - LM Studio (OpenAI-compatible)
- Support for multiple programming languages:
  - Python (Google/NumPy/Sphinx docstrings)
  - JavaScript/TypeScript (JSDoc)
  - Go (standard comments)
  - Rust (doc comments)
  - Java
  - C/C++
- Comprehensive documentation:
  - README with quick start guide
  - Technical design document
  - Configuration examples
  - Docker deployment guide

### Security
- API token storage via environment variables
- Webhook signature validation
- Rate limiting considerations
- Sanitized LLM prompts to prevent injection

[Unreleased]: https://github.com/slb350/drep/compare/v2.2.0...HEAD
[2.2.0]: https://github.com/slb350/drep/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/slb350/drep/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/slb350/drep/compare/v1.3.0...v2.0.0
[1.3.0]: https://github.com/slb350/drep/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/slb350/drep/compare/v1.1.3...v1.2.0
[1.1.3]: https://github.com/slb350/drep/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/slb350/drep/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/slb350/drep/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/slb350/drep/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/slb350/drep/compare/v0.9.0...v1.0.0
[0.9.0]: https://github.com/slb350/drep/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/slb350/drep/compare/v0.1.0...v0.8.0
[0.1.0]: https://github.com/slb350/drep/releases/tag/v0.1.0
