# OpenAI API and ChatGPT/Codex subscription integration plan

- Status: implemented and release-qualified for v2.3.0
- Branch: `codex/openai-subscription-plan`
- Base: `v2.2.0` / `3067e9c219f7e38e00744eec6b9f5eb1fd926bca`
- Research date: 2026-08-20

## Implementation outcome

The design below was implemented without changing the existing OpenAI API
preset or billing path. The initial implementation passed a live
`gpt-5.6-sol` ChatGPT subscription smoke; the default remains one concurrent
Codex process because no higher plan-safe concurrency was established.

After an independent Kimi review, every accepted finding was reproduced or
validated before repair. The final branch passed formatting, Clippy, the full
Rust suite, strict documentation lint, a release build, a 1,124-mutant sweep
with zero misses, and the normal Kimi-backed pre-push review. The final 2.3.0
release candidate repeated the full sweep over 1,127 mutants with zero misses.
The final `drep 2.3.0` binary also reviewed two uncached Python files through
the ChatGPT subscription backend in 15.78 seconds: `gpt-5.6-sol` served both,
the command exited zero, and it reported no findings or unanalyzed files. The
OpenAI API path has deterministic wire-contract coverage; no paid OpenAI API
credential was available for an additional live request.

The implementation commits are `7642572` (backend), `3f0def6` (simplification)
and `1b4fa50` (validated review fixes). This document retains the original
design and TDD sequence as the rationale for the shipped boundary.

## Decision summary

drep already supports the OpenAI API. The existing `openai` preset sends an
OpenAI-compatible Chat Completions request to `https://api.openai.com/v1` with
`OPENAI_API_KEY`, defaults to `gpt-5.6-sol`, and omits `temperature`, which that
model rejects. OpenAI currently documents `gpt-5.6-sol` as supporting both
`v1/chat/completions` and `v1/responses`, so API support does not need to be
rebuilt or migrated to Responses as part of this feature.

ChatGPT/Codex subscription usage is a separate backend, not another HTTP
endpoint or API-key preset. The `codex` preset invokes the installed
Codex CLI in non-interactive mode, forces ChatGPT authentication, and consumes
the user's Codex subscription allowance. drep will not read, copy, refresh, or
store ChatGPT OAuth tokens.

The selected integration surface is `codex exec`, not app-server:

- OpenAI documents `codex exec` for pipelines, CI, pre-merge checks, piped
  input, explicit sandboxing, JSONL events, ephemeral sessions, and structured
  output. That is drep's exact workload.
- The installed app-server currently inherits user configuration, global
  instructions, skills, and MCP servers. A measured minimal request carried
  22,200 input tokens and initialized the user's MCP servers. Its CLI does not
  expose `--ignore-user-config`.
- A locked-down `codex exec` request can ignore user configuration, replace
  built-in instructions, disable tool surfaces, enforce ChatGPT auth, and run
  from an empty temporary directory. The measured minimal request carried
  10,849 input tokens and emitted only the expected lifecycle and final-message
  events.

App-server is not a planned second implementation. Reconsider it only if it
gains an isolation contract equivalent to `--ignore-user-config` and a measured
multi-file run materially outperforms the locked-down exec backend.

## Baseline evidence

### Release and branch base

- Local `main`, `origin/main`, `github/main`, and local/remote `v2.2.0` all
  resolved to `3067e9c219f7e38e00744eec6b9f5eb1fd926bca` after fetching both remotes.
- `Cargo.toml` reports `2.2.0`, and `CHANGELOG.md` has the 2026-08-20 entry.
- The GitHub v2.2.0 release and all release workflow jobs completed
  successfully. The release contains four platform archives, checksums,
  installers, source archives, the Homebrew formula, and the release manifest.
- The public Homebrew tap formula reports 2.2.0. The local tap checkout still
  reports 2.1.0; that is stale local Homebrew metadata, not a release failure.

### Existing OpenAI API support

The current path is already end to end:

1. `src/cli/init/presets.rs` declares `openai`, the official base URL,
   `gpt-5.6-sol`, and `OPENAI_API_KEY`.
2. `protocol` defaults to `openai`, which resolves to the SDK's
   `ApiProtocol::OpenAiChat`.
3. `LlmClient` sends bearer-authenticated streaming requests to
   `/chat/completions`, assembles text events, applies drep's retry rules, and
   feeds the existing JSON extraction ladder.
4. Preset, protocol, bearer-header, request-shape, streaming, retry, failover,
   and cache-key behavior have local tests.

No OpenAI API credential is present in the current environment or drep auth
store, so this research did not perform a paid live API request. Implementation
must add an OpenAI-specific wire contract test and document an opt-in live
smoke, but the feature is not blocked on a network test in CI.

### Subscription research and measurements

The local Codex CLI is 0.148.0. Its redacted diagnostic reports ChatGPT-managed
authentication, and the final locked-down smoke proved that the saved session
works with `gpt-5.6-sol`, JSONL output, and an output schema.

Minimal one-line structured-output measurements:

| Invocation | Input tokens | Finding |
| --- | ---: | --- |
| `codex exec` from this repository | 28,532 | Project instructions leaked into the request. |
| `codex exec` from an empty cwd | 20,314 | Neutral cwd removed repository instructions. |
| Empty cwd plus `project_doc_max_bytes=0` | 20,312 | This setting alone did not remove the generic agent context. |
| App-server with replacement base instructions | 22,200 | User config, instructions, skills, and MCPs loaded. |
| Empty cwd plus `model_instructions_file` | 16,252 | Replacement instructions reduced generic context. |
| Fully locked-down exec contract | 10,849 | Tools/integrations disabled and ChatGPT auth forced. |

The remaining fixed context is material for a per-file gate. Subscription
support therefore needs a full-branch quota and throughput qualification before
release; a successful one-line smoke is necessary but not sufficient.

## Scope

### In scope

- Preserve and explicitly document the existing OpenAI API mode.
- Add a distinct ChatGPT/Codex subscription preset and backend.
- Guarantee that selecting subscription mode cannot silently use an API key.
- Keep subscription credentials owned by Codex.
- Integrate subscription responses with the existing cache, limiter, failover,
  result parser, terminal output, JSON output, and `doctor` report.
- Provide deterministic fake-Codex tests plus an opt-in live subscription smoke.
- Qualify subscription quota use, latency, concurrency, and failure behavior on
  a representative multi-file review.

### Out of scope

- Migrating the existing OpenAI API backend from Chat Completions to Responses.
- Adding Responses as a third direct HTTP wire protocol for every compatible
  provider.
- Importing ChatGPT OAuth tokens into drep's auth store.
- Copying or parsing `~/.codex/auth.json`.
- Supporting external-token app-server authentication.
- Running the Codex SDK through a Node.js or Python sidecar.
- Remote/cloud Codex tasks, interactive threads, resume, or persisted history.
- Allowing Codex tools to inspect or modify the repository during a review.

## User-facing configuration

Add a backend discriminator to each `[[llm]]` entry:

```toml
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
timeout_secs = 1800
max_concurrent = 1
```

Rules:

- An absent `backend` means `http`, preserving every existing drep.toml.
- `backend = "http"` keeps the current endpoint/API-key/protocol behavior.
- `backend = "codex"` requires `model` and accepts
  `reasoning_effort`, `timeout_secs`, and `max_concurrent`.
- A Codex entry rejects `endpoint`, `api_key`, `protocol`, `temperature`,
  `max_tokens`, and `max_retries` when explicitly present. Ignoring those fields
  would make the file claim controls that the backend does not honor.
- Disabled entries remain inert. As today, their credentials and backend-only
  fields are not resolved or validated until re-enabled.
- The initial Codex preset writes `max_concurrent = 1`. Raise it only if the
  concurrency qualification demonstrates better throughput without increased
  failures or disproportionate plan consumption.

Keep the existing preset key `openai` for compatibility, but change its display
name to `OpenAI API`. Add `codex` with display name
`ChatGPT / Codex subscription`. The wizard and `--provider` help must never use
the bare label `OpenAI` for both choices.

Implementation should deserialize optional/raw fields first and validate them
into a typed backend enum. Downstream code must not carry an HTTP client with a
fake endpoint for Codex or a Codex client with ignored HTTP fields.

## Runtime design

### Backend boundary

Keep enum dispatch inside Rust rather than adding an async-trait dependency:

```text
ProviderBackend
├── Http(LlmClient)
└── Codex(CodexClient)
```

The backend exposes only the operations the chain needs:

- model name;
- stable display and cache identity;
- optional sampling identity for the existing cache contract;
- `complete_json(system_prompt, user_content)`;
- backend-specific diagnostic status.

`Provider`, `ProviderChain`, limiter ownership, served counts, sticky demotion,
and one-provider error collapsing remain shared. The HTTP implementation and
its tests should move only as much as the enum boundary requires.

### Locked-down Codex command

For each uncached file, create a private temporary directory containing:

- `instructions.md`: the existing language-specific drep review prompt plus an
  explicit no-tools/no-files instruction;
- `schema.json`: a strict JSON Schema for drep's current `issues` and `summary`
  response shape.

Then write `user_content` to the child's stdin and invoke the equivalent of:

```sh
codex \
  -c 'forced_login_method="chatgpt"' \
  -c 'model_instructions_file="<temp>/instructions.md"' \
  -c 'model_reasoning_effort="<configured>"' \
  -c 'project_doc_max_bytes=0' \
  -c 'web_search="disabled"' \
  --disable shell_tool \
  --disable unified_exec \
  --disable apps \
  --disable multi_agent \
  --disable hooks \
  --disable memories \
  -a never \
  exec \
  --ephemeral \
  --ignore-user-config \
  --ignore-rules \
  --sandbox read-only \
  --skip-git-repo-check \
  -C <empty-temp-directory> \
  --model <configured-model> \
  --output-schema <temp>/schema.json \
  --json \
  -
```

All arguments are load-bearing:

- `forced_login_method` makes API-key fallback impossible.
- `model_instructions_file` replaces the generic coding-agent and AGENTS.md
  instructions with drep's review contract.
- user config, project rules, web search, apps, subagents, hooks, memories,
  shell, and unified execution are excluded.
- the empty cwd is outside the reviewed repository and contains no project
  instructions or source files.
- read-only plus never-approve are defense in depth if Codex adds a tool in a
  later release.
- ephemeral mode prevents one review thread per file from polluting history.
- the strict output schema reduces format drift, while the existing parser
  remains the authority on severity, required fields, and line provenance.

Construct TOML override values with a real TOML string encoder. Do not hand-
quote paths; spaces, backslashes, and quotes must work on every release target.

Resolve `codex` through PATH in production. The executable path is injected
into `CodexClient` in tests; no test changes process PATH or environment.

### Child environment and credential boundary

Build an allowlisted environment for the Codex child instead of forwarding the
entire drep environment. Preserve only values required to locate the binary,
the OS user home/Codex home, the temporary directory, certificates/proxies, and
platform runtime basics. Explicitly strip `OPENAI_API_KEY`, `CODEX_API_KEY`, all
provider API-key variables, and drep test overrides.

Codex reads and refreshes its own saved ChatGPT session. drep never reads the
credential file, never copies it to a temporary directory, and never places a
token in an argument, environment variable, log, cache key, debug formatter, or
error message.

Run `codex doctor --json` once while building a Codex provider to give an early,
redacted diagnostic. Unknown diagnostic JSON fails closed with an actionable
"unsupported Codex CLI diagnostic format" message. The runtime auth guarantee
does not depend on that parser: `forced_login_method="chatgpt"` is passed to
every actual review command.

### Process lifecycle and output

- Use Tokio's process API with piped stdin/stdout/stderr and `kill_on_drop`.
- Apply `timeout_secs` to the entire child lifecycle, including stdin write and
  process exit. On timeout, kill and reap the child before returning.
- Bound captured stdout and stderr. A protocol stream larger than the limit is
  a backend failure, not an unbounded allocation.
- Parse stdout as JSONL. Ignore progress events, accept the final
  `item.completed` whose item is `agent_message`, and require a later
  `turn.completed`.
- Reject command/file/MCP/web/subagent events. They indicate that the isolation
  contract regressed, even if a final answer follows.
- Keep stderr only as a bounded, control-character-free diagnostic excerpt.
- Parse the final message as JSON and return `Extracted::Complete`. Structured
  output should make repair unnecessary; malformed output remains
  `Unparseable` and is never silently accepted.

### Errors, failover, and sticky demotion

Do not classify errors by matching prose.

- Missing binary, unsupported flags/schema, non-ChatGPT auth, invalid event
  protocol, or a tool event: configuration/backend-contract failure. Stop the
  chain and remember it for the run, like a bad API credential.
- Spawn failure after resolution, broken pipe, timeout, premature EOF, or a
  child terminated by signal: transport failure. Fail over and demote.
- A documented structured transient error, when present in JSONL: fail over
  and demote.
- Usage-limit exhaustion: fail over and demote for the run. Repeating it for
  every file cannot help.
- Unauthorized/authentication failure: stop the chain and demote. Falling back
  would hide that the selected subscription is not usable.
- Bad request, context-window exhaustion, schema refusal, content policy, or
  non-empty malformed final JSON: request-shaped failure. Do not fail over or
  demote.
- Unknown nonzero exits stop the chain without prose-based guessing. Add a new
  typed mapping only after a captured fixture proves the event shape.

The one-provider rule still applies: a single Codex entry surfaces its own
failure reason rather than wrapping it in `ChainFailed`.

### Cache identity

Never pretend Codex has `https://api.openai.com/v1` as its endpoint. Introduce
a backend identity used by display and cache code.

The Codex cache key must distinguish at least:

- backend kind (`codex` versus `http`);
- forced auth mode (`chatgpt`);
- model;
- reasoning effort;
- Codex CLI version;
- drep system prompt and user payload, as today.

Including the CLI version deliberately invalidates cached answers when Codex's
hidden execution contract changes. Do not include account email, workspace ID,
plan name, auth paths, tokens, or other personal data.

## Initialization and diagnostics

### `drep init`

- `drep init --provider openai` remains non-interactive and writes the existing
  API configuration.
- `drep init --provider codex` writes the backend block above without asking
  for or storing a key.
- Interactive init checks that `codex` is executable and that the redacted
  diagnostic reports ChatGPT auth. If not, it explains how to install Codex or
  run `codex login`; drep does not launch a browser or alter Codex auth itself.
- `--model` continues to override the preset default.
- Do not use HTTP `/models` for the Codex preset. The chosen model is validated
  by the real locked-down smoke/first run. Adding app-server only to list models
  would reintroduce the rejected integration surface.
- The renderer explains that usage is charged against the ChatGPT/Codex plan,
  not OpenAI API billing.

### `drep auth`

`drep auth` remains API-key-only. Passing `--provider codex` should explain
that Codex owns subscription authentication and point to `codex login`; it must
not create an endpoint key or inspect OAuth state.

### `drep doctor`

For an HTTP provider, retain the existing endpoint/model/key-source report. For
a Codex provider, show only redacted, actionable state:

```text
1. gpt-5.6-sol via ChatGPT/Codex subscription
   codex: found (0.148.0)
   authentication: ChatGPT-managed
   isolation: ephemeral, tools disabled
```

Do not print an account email, workspace ID, credential path, raw diagnostic
JSON, or plan balance. `doctor` remains non-failing; `check` remains the gate.

## TDD implementation sequence

Each phase starts with a failing test, implements only enough to make it pass,
then runs formatter, Clippy, the relevant suite, and the mutation gate required
by the repository.

### Phase 1: configuration and preset contract

RED tests:

- old configs with no `backend` load as HTTP unchanged;
- `openai` retains its exact API endpoint/model/key environment/protocol;
- `codex` is a separate preset with no endpoint or API-key environment;
- Codex-only and HTTP-only fields validate exactly as specified;
- disabled entries remain inert;
- unknown backend and reasoning effort name their table in file order;
- debug output cannot expose credentials.

Implementation:

- add raw-to-validated backend configuration;
- refactor init choices/rendering to support endpoint-less Codex entries;
- rename only the OpenAI display label, not its stable preset key.

### Phase 2: backend boundary and unchanged HTTP proof

RED tests:

- the same API config still builds `LlmClient` and sends the same request;
- an OpenAI-specific mock receives `/v1/chat/completions`, bearer auth,
  `gpt-5.6-sol`, no temperature, and no invented max-token cap;
- backend identities and cache keys cannot collide;
- same-model HTTP/Codex failover attributes cache entries to the serving
  backend.

Implementation:

- add `ProviderBackend` enum dispatch;
- preserve HTTP retry/parsing code behind the existing client;
- replace endpoint-only cache/display assumptions with backend identity.

### Phase 3: Codex command construction and isolation

RED tests use a fake executable written with
`test_support::write_executable`:

- exact ordered arguments, stdin payload, cwd, and allowlisted environment;
- ChatGPT-only auth override is always present;
- instructions and schema files have the expected content;
- every forbidden tool/config surface is disabled;
- temp paths with spaces and quotes are TOML-encoded correctly;
- no real Codex home, auth file, repository, or process environment is read by
  a test.

Implementation:

- add `src/llm/codex.rs` plus focused submodules/tests before any file reaches
  the 600-line soft limit;
- promote the existing `tempfile` dependency from dev-only to runtime use;
- add bounded child IO, timeout, kill, and reap handling.

### Phase 4: JSONL, schema, and error classification

RED fixture tests cover:

- successful lifecycle and final structured agent message;
- fragmented JSONL reads;
- progress events before the final message;
- missing turn completion, duplicate final messages, malformed/oversized lines,
  bounded stderr, nonzero exit, signal, timeout, and broken stdin;
- forbidden command, file, MCP, web, and subagent events;
- each proven typed transient/auth/request error mapping;
- unknown errors stopping rather than guessing from prose.

Implementation:

- parse owned event types, ignoring unknown fields for forward compatibility;
- keep unknown event *types* diagnostic and fail closed when they could change
  the review result;
- route typed outcomes through existing failover/sticky rules.

### Phase 5: init, auth, doctor, and documentation

RED tests cover:

- flag and wizard rendering for both OpenAI choices;
- no key prompt/store mutation for Codex;
- actionable missing-binary, API-auth-mode, and unknown-diagnostic messages;
- redaction of email, paths, and diagnostic details;
- README examples make API billing versus subscription allowance explicit.

Implementation:

- wire preset, init, auth guidance, and doctor output;
- update README, technical design, changelog, shell completion/help snapshots,
  and AGENTS.md invariants.

### Phase 6: integration, mutation, and qualification

Automated gates:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-targets --all-features
./scripts/mutants-run.sh
./target/release/drep lint-docs --fail-on error
```

Manual/opt-in gates:

1. OpenAI API smoke with an explicitly supplied test credential; verify a
   clean structured response and no temperature field. Never make this a
   credentialed hosted-CI requirement.
2. ChatGPT subscription smoke with a dedicated tiny fixture; verify
   ChatGPT-only auth, no tool events, ephemeral history, and structured output.
3. Full feature-branch review through the subscription backend. Record file
   count, wall clock, per-turn input/output usage, failures, and plan-limit
   behavior.
4. Repeat the representative batch at concurrency 1, 2, and 3. Keep 1 unless a
   higher setting improves throughput without more failures or materially
   greater plan consumption.
5. Run the same branch through the existing HTTP backend and confirm findings,
   gating, cache, failover, and output formats did not regress.

Do not release if a Codex run emits a tool event, can use API-key auth, loads
repository/user instructions, leaves persisted sessions, leaks child stderr or
account data, or makes a normal multi-file gate impractical under the measured
subscription allowance.

## Compatibility and release notes

- Configuration is backward compatible because `backend` defaults to `http`.
- The `openai` preset key remains stable.
- Subscription support requires a separately installed Codex CLI with the
  locked-down flags/config keys validated during implementation. Report the
  detected version rather than silently weakening isolation for an older CLI.
- drep's binary remains standalone for every HTTP backend. Only users who
  select `backend = "codex"` acquire the external Codex dependency.
- Treat this as a minor feature release. Do not claim API support is new; say
  that direct OpenAI API support was clarified/tested and ChatGPT/Codex
  subscription support was added separately.

## Primary sources

- [Codex authentication](https://learn.chatgpt.com/docs/auth)
- [Codex non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
- [Codex configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [GPT-5.6 Sol model endpoints](https://developers.openai.com/api/docs/models/gpt-5.6-sol)
