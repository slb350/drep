# drep 2.0 — Rust rewrite migration plan

Branch: `rust-rewrite`. Target: a single multi-arch binary installable via
`brew install slb350/tap/drep` or direct download, that gates commits and
pushes locally.

This is a rewrite, not a port with a compatibility window. Python `drep/` stays
on the branch as reference material and is deleted in the final phase. There is
no deprecation path, no dual-maintenance period, and no config migration.

## What drep 2.0 is

A local pre-commit / pre-push gate that:

1. Runs whatever linters and formatters the repository has **configured**
   (ruff, eslint, tsc, gofmt, go vet, clippy) — their findings **block**.
2. Sends the **changed code** to an LLM for review — its findings **inform**
   unless `--fail-on` opts in.

Four commands:

```
drep check [PATHS | --staged | --diff <ref>]  --format text|json  --fail-on <sev>
drep lint-docs [PATHS]
drep doctor
drep init
```

Two triggers: pre-commit and pre-push. Nothing else.

## What is dropped

| Dropped | Python LOC | Why |
|---|---:|---|
| `adapters/` (gitea, github, gitlab) | 2,724 | Platform integration is a crowded market; not our differentiator |
| `server.py` + FastAPI/uvicorn | 194 | Webhooks existed to serve the adapters |
| `pr_review/analyzer.py` | 350 | Comment posting dies with the adapters; `diff_parser.py` survives |
| `db/` (`RepositoryScan`, `FindingCache`) | 124 | Issue dedup is meaningless without issues; incremental-scan SHA is meaningless when diffing git directly |
| `core/issue_manager.py` | 116 | Same |
| `docstring/` | 496 | Generation (not reporting) is why it needed `ast`. See "Deliberate behavior changes" |
| `cli_wizard.py` + `models/wizard.py` | 789 | Mostly platform tokens and URLs; what remains is `init-llm` |
| `llm/providers/bedrock_client.py` | 316 | open-agent-sdk-rust is OpenAI-compatible-endpoints only. Bedrock users point at a gateway (LiteLLM) |
| `llm/metrics.py` + `metrics` command | 345 | Replaced by a `--stats` flag printing tokens/cost for the run. No history store |
| `scan`, `review`, `serve`, `validate` commands | — | No platform, no server |

Roughly **5,400 LOC deleted** of 13,343. Dependencies dropped: sqlalchemy,
fastapi, uvicorn, uvloop, gitpython, boto3/botocore, pydantic, click, httpx.

What survives and gets ported is ~4,500 LOC of real substance, which lands
around 6,000–8,000 LOC of Rust with tests. That is the actual size of this job.

## Repo layout during the migration

Rust lives at the repo root alongside the Python package. Python is
reference-only from Phase 1 onward — no further changes to it.

```
Cargo.toml           # new
src/                 # new
tests/*.rs           # new (integration tests, alongside the Python suite)
drep/                # Python, reference only, deleted in Phase 8
tests/               # Python, reference only, deleted in Phase 8
pyproject.toml       # deleted in Phase 8
```

Rust integration tests go directly in `tests/` as `tests/*.rs`, not a `tests/rust/`
subdirectory: cargo only discovers test targets at `tests/*.rs` (or
`tests/<dir>/main.rs`), so a nested `tests/rust/cli.rs` would silently never
run. The two suites coexist there until Phase 8 - pytest collects only `.py`,
cargo only `.rs`.

## Module map

| Rust | Ported from | Notes |
|---|---|---|
| `src/main.rs` | `cli.py` | clap entry point |
| `src/cli/check/` | `cli_workflows.py::_run_check` | Exit codes 0/1/2; `input`/`deterministic`/`render` |
| `src/cli/lint_docs.rs` | `cli.py::lint_docs` | |
| `src/cli/doctor.rs` (tests in `doctor/`) | `cli_doctor.py` | Language/tool detection |
| `src/cli/init/` | `cli_init_hooks.py` + init-llm | `presets`/`config_file`/`hooks`; writes native git hooks |
| `src/languages/spec.rs` | `languages/base.py` | `ToolSpec`, `LanguageSupport`, registry |
| `src/languages/definitions.rs` | `languages/definitions.py` | The language table |
| `src/languages/runner/` | `languages/runner.py` | Tool resolution + execution, and the parsers |
| `src/languages/runner/parsers.rs` | `languages/runner.py` parsers | json / tsc / lines / position / cargo |
| `src/files/mod.rs` | `core/file_targets.py` | `ignore` crate for the walk |
| `src/diff/mod.rs` | `pr_review/diff_parser.py` + `llm/git_utils.py` | `--staged` and `--diff <ref>`; shells out to git |
| `src/llm/client/` | `llm/client.py` | Thin over `open-agent-sdk` |
| `src/llm/cache.rs` | `llm/cache.py` | Content-addressed, keyed on hunks |
| `src/llm/concurrency.rs` | `rate_limiter.py` + `circuit_breaker.py` | Deliberately simplified |
| `src/llm/json_parsing.rs` | `llm/json_parsing.py` | Tolerant JSON extraction |
| `src/analysis/code_quality.rs` | `code_quality/analyzer.py` | |
| `src/analysis/findings.rs` | `models/findings.py` | `Severity`, `SEVERITY_RANK` |
| `src/docs/markdown.rs` | `documentation/analyzer.py` | Includes `_fence_mask` |
| `src/config.rs` | `config.py` | TOML, not YAML |

## Dependencies

Versions verified 2026-08-17.

```toml
[dependencies]
open-agent-sdk = "0.7.0"    # crates.io; docs at https://docs.rs/open-agent-sdk
tokio          = { version = "1.53", features = ["rt-multi-thread", "macros", "process", "fs", "sync", "time"] }
clap           = { version = "4.6", features = ["derive"] }
serde          = { version = "1.0", features = ["derive"] }
serde_json     = "1.0"
toml           = "1.1"
anyhow         = "1.0"
thiserror      = "2.0"
ignore         = "0.4"      # gitignore-aware parallel walk
blake3         = "1.8"      # cache keys
directories    = "6.0"      # cache/config dirs
tracing        = "0.1"
futures        = "0.3"

[dev-dependencies]
assert_cmd = "2.2"
insta      = "1.48"         # snapshot tests for output formats
wiremock   = "0.6"          # mock LLM endpoint
tempfile   = "3.27"
```

Edition 2024, `rust-version = "1.85"` (floor set by the SDK).

## Working agreement (how these phases get built)

Implementation is delegated to MiniMax M3 via `opencode run`; Claude writes the
specification and adversarially verifies the result. The global
`delegate-and-verify` skill has the full procedure. The parts specific to this
repo:

- **Paste every dependency API into the spec.** The agent is sandboxed to
  `--dir` and cannot read `~/.cargo/registry`; an external read is auto-rejected
  and the run dies inside a minute. Two runs died that way (`ignore 0.4`,
  `toml 1.x`). Verify the API from the installed source yourself first.
- **Add dependencies yourself**, and forbid the spec from touching the manifest.
- **Write acceptance criteria that discriminate.** A criterion satisfiable by a
  wrong implementation manufactures confidence. Every real defect across five
  phases was in the *verification*, not the code: orphaned test files, a
  three-dot test that passed under two-dot, retry tests that bypassed the
  production path, fenced-JSON tests that only used single-line bodies.
- **Verify by breaking the implementation**, not by reading the tests. Invert the
  behaviour a test claims to pin and confirm that specific test fails.
- **`cargo mutants` is the systematic version of that** and gates both
  pre-commit and CI. A surviving mutant *is* a non-discriminating test. Fix it by
  making a test discriminate, never by excluding the mutant.
- **Distrust prose the agent writes.** It has claimed verifications it did not
  run, including one describing the inverse of the correct check.
- **Never run it concurrently with anything that reads the working tree** — a
  `git push` whose hook analyzes files will read half-written ones.
- **Never pipe its output through `tail`**; that buffers until EOF, so a hung run
  looks identical to a quiet one. Redirect to a file and poll.
- **Run the real binary against this repository before believing the phase is
  done.** See "Ground truth is not optional" below; it is the step that found
  the two most serious defects of the rewrite so far.

### Ground truth is not optional

Phase 5b's suite was green at 353 tests, clippy-clean and mutation-clean. drep
gating its own push then found two defects the suite could not reach, one of
them four phases old.

**`cargo clippy` rejects file arguments**, so the deterministic half for Rust
had never run since Phase 1. The parser tests feed captured clippy output
straight to `parse_output`, and the `run_tool` tests use stub executables that
accept any argv. Neither can say whether the *real* binary accepts the argv
drep builds.

**An empty LLM response was classified as a deterministic parse failure** and
therefore never retried. wiremock returns exactly what it is told, so it cannot
represent a provider that *intermittently* returns nothing — which is precisely
the failure mode.

The pattern generalises: a test double answers the question you designed it to
answer. Anything about how a real external program behaves — its argument
grammar, its exit codes, its bad days — is only observable by running it.

## Phases

TDD throughout: failing test, implement, refactor, commit. Every phase ends
green with `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`.

### Phase 0 — Skeleton
`Cargo.toml`, `src/main.rs` with a clap skeleton for all four commands (each
`todo!()`), CI workflow running fmt/clippy/test on macOS + Linux. Ends with
`drep --version` working.

### Phase 1 — `languages/` (do this first)
The crown jewel, and the only major subsystem with **no LLM dependency** —
fully testable offline, which makes it the right place to establish testing
patterns.

Port `ToolSpec`/`LanguageSupport`/registry, the five language definitions, tool
resolution, subprocess execution, and **five** output parsers — `json`, `tsc`,
`lines`, `position` (go vet), `cargo` (clippy). `json` alone covers two
unrelated shapes: ruff's flat records with a `location`, and eslint's one
record per file with a nested `messages` array.

`Finding` lands here too, not in Phase 4 as first written: the tool parsers
produce findings, so the type cannot wait for the LLM path.

Ends with `drep check --no-llm PATHS` producing blocking findings from real
ruff/eslint/gofmt runs against fixture repos.

### Phase 2 — File targeting and diff ✅
`src/files/` on the `ignore` crate (replaces the hand-rolled walk + prune;
gitignore-awareness comes free). `src/diff/` for `--staged` and `--diff <ref>`,
shelling out to `git`. Ends with correct file sets for all three input modes,
deduped. **Landed 2026-08-17** — see CHANGELOG for the full note (30 new tests,
`src/files/{mod,tests}` and `src/diff/{mod,tests}` follow the directory-module
pattern from Phase 1, `ignore = "0.4"` is the only new dependency).

### Phase 3 — LLM layer ✅
Wire `open-agent-sdk` (0.7.0, crates.io, edition 2024, rust-version 1.85 — it
is what sets this crate's MSRV). Port the cache (content-addressed on prompt +
content + model + temperature), tolerant JSON parsing, and a simplified
concurrency limiter. Test against `wiremock`, not a live endpoint.

The SDK published 2026-08-09, so it post-dates the training cutoff of any model
generating code against it, and it is not in the ref-context index either. The
API surface below was read from the crate source at
`~/.cargo/registry/src/*/open-agent-sdk-0.6.9/`; paste it into any delegated
spec, because a sandboxed agent cannot reach that path.

**The crate is `open-agent-sdk` but the library is `open_agent`** — `[lib] name =
"open_agent"`, so it is `use open_agent::...`, never `open_agent_sdk`.

```rust
use open_agent::{AgentOptions, ContentBlock, Error, query};
use open_agent::retry::{RetryConfig, retry_with_backoff_conditional};

let options = AgentOptions::builder()
    .model("...")            // required
    .base_url("...")         // required
    .api_key("...")
    .system_prompt("...")
    .temperature(0.2_f32)
    .timeout(1800_u64)       // seconds
    // .max_tokens(n)        // OMIT to send no cap - see below
    .build()?;               // -> Result<AgentOptions>

let mut stream = query(prompt, &options).await?;   // ContentStream
// ContentStream = Pin<Box<dyn Stream<Item = Result<ContentBlock>> + Send>>
// ContentBlock = Text(TextBlock) | Image | ToolUse | ToolResult
```

Two things the SDK already gets right, so **do not reimplement them**:

- **`max_tokens` omission works as of 0.7.0.** `AgentOptions::max_tokens()`
  returns `Option`, and the builder now leaves the field out of the request when
  the setter is never called. (0.6.9 substituted 4096, which silently truncated
  reasoning models; drep carried a large sentinel to compensate. That workaround
  is deleted.)
- **The retry taxonomy already exists.** `Error::Api` carries
  `status: Option<u16>`, and `retry::is_retryable_error` classifies against
  `RETRYABLE_STATUS_CODES = [408, 429, 500, 502, 503, 504, 529]` plus `Http`,
  `Timeout` and `Stream`. `retry_with_backoff_conditional(config, op)` applies it
  with exponential backoff and jitter; `RetryConfig::default()` is 3 attempts, 1s
  initial, 60s max, 2.0 multiplier, 0.1 jitter.

  Construct HTTP errors with `Error::api_status(code, msg)`, not `Error::api(msg)`
  - the latter leaves `status: None` on purpose, so it is never retryable.

That leaves drep responsible for only the *other* half of the taxonomy: a
truncated response is **not** an SDK error. The stream completes successfully and
the damage surfaces as unparseable JSON in drep's own parsing. So transport
retries belong to the SDK, and a parse failure must fail the file without a
retry — which is exactly the split the Python conflated.

**Retry policy must discriminate by failure class.** The Python has one
`max_retries` governing every failure, which forces a bad trade and is why 7 of
28 files went unanalyzed on the first real gated push:

| Failure | Deterministic? | Retry? |
|---|---|---|
| Response truncated at `max_tokens`, unparseable JSON | Yes - repeats identically | **No.** Re-burns a full reasoning-model call for the same result |
| 429, 5xx, connection reset, timeout | No - transient | **Yes.** A failed request consumes no tokens, so a retry is nearly free |

`config.yaml` sets `max_retries: 1` with the comment "a 'length' failure repeats
deterministically; retrying just burns tokens" - correct for the first row, and
it silently disables the second. Split the two in `llm/client.rs`: transport
errors retry with backoff, length and parse errors fail the file immediately.

Keep the existing invariant while doing it: `max_retries` is a **total attempt
count with a floor of 1**. Config permits 0, and a bare `0..max_retries` loop
would skip the request entirely and then report a bogus "no exception was
captured".

**`max_tokens` defaults to unset, and never feeds the rate limiter.** In 1.x one
number does two jobs that want opposite values:

- As the **API completion cap** it wants to be generous. Reasoning tokens count
  against it, so a cap that is too small truncates the model mid-thought and
  yields unparseable output — the deterministic length failure above.
- As the **rate-limit reservation** (`(prompt + code + max_tokens) / 4`) it wants
  to be tight. It is a worst-case guess, and overestimating throttles requests
  that were never going to use it.

Coupling them means every increase in reasoning headroom silently buys a
throughput cut. Shipped 1.3.0 demonstrates it: `drep init-llm --provider
openrouter` writes `max_tokens: 100000` and does *not* write
`max_tokens_per_minute`, which inherits the 100,000 default — so one 13KB file
reserves 28,845 tokens and the preset throttles itself to **3 requests per
minute** out of the box.

In 2.0 (**landed in Phase 3a**):

- **No cap is sent unless the user sets one.** Models ship 256k–1M context;
  inventing a ceiling is drep's bug, not the model's limit. `LlmClient` forwards
  `LlmConfig::max_tokens` only when it is `Some`, and open-agent-sdk 0.7.0 omits
  the field otherwise.
- **Reserve against an expected completion size, not the cap** — a modest
  constant, or the observed rolling average. This is safe because the limiter
  already does two-phase accounting: it reserves an estimate on entry and
  reconciles actual usage on exit, so the forward guess only has to prevent a
  stampede. Preserve that reconcile step.

  **Superseded by Phase 3b:** the limiter ended up as a bare concurrency cap
  with no token budget, so there is nothing to reserve and nothing to reconcile.
  429 is handled by the SDK's retry. If a token budget is ever reintroduced,
  use `open_agent::estimate_tokens(&[Message])` rather than re-deriving a
  chars-per-token heuristic.

- A user-set `max_tokens` is sent to the API but must **not** inflate the
  reservation.

**Landed 2026-08-17** as 3a (config, client, JSON extraction) and 3b (cache,
concurrency). Requires open-agent-sdk **0.7.0**. See CHANGELOG for the full note.

### Phase 4 — Analysis and exit codes ✅
Split into 4a and 4b: together they exceed the size where a single delegated
handoff stays reliable, and 4a is fully offline while 4b is not.

**Phase 4a ✅ — hunks and payload. Landed 2026-08-17.** `src/diff/hunks.rs`
(parser), `staged_hunks`/`hunks_since` in `src/diff/mod.rs` at
`--unified=20`, and `src/analysis/payload.rs`. See the CHANGELOG for the full
note. `findings.rs` already carried `Severity` and `Finding` from Phase 0, so
the phase list's mention of it was stale.

**Phase 4b ✅ — analysis and the failure contract. Landed 2026-08-17.**
`src/analysis/{prompt,result,code_quality}.rs`. The five-level LLM scale lives
on `LlmSeverity` beside `Severity` in `findings.rs`, and the prompt renders its
alternation from `LlmSeverity::ALL` so the levels asked for and the levels
parsed are the same list. See the CHANGELOG for the full note.

Two rules for 4b that are decisions, not ports:

- **`Extracted::Truncated` records the file in `failed_files` *and* returns its
  partial findings.** Unconditionally — the analysis layer never consults
  `--fail-on`. Phase 5's CLI decides what a failed file maps to. Conditioning it
  on the flag would make `failed_files` flag-dependent, so the JSON `unanalyzed`
  field would mean different things per invocation.
- **A finding whose line is not in `Payload::valid_lines` is dropped, not
  clamped** — it is about code the model was never shown. Dropped findings are
  counted so the drop is observable rather than silent. A *malformed* finding
  record (unknown severity, missing field, a line beyond `u32`) is different: it
  makes the file unanalyzed, because we cannot know what it said.

**Decided in Phase 5 (5a and 5b):** `failed_files` became
`BTreeMap<PathBuf, FailureReason>`, and `--format json`'s `unanalyzed` entries
carry a stable `kind` tag plus a numeric `status` for HTTP failures. Exit code
2 never needed the reason; failover does, and it must not have to match on
prose to get it.

### Phase 5 — CLI assembly (5a ✅, 5b ✅, 5c ✅)
Split three ways. 5a is where every subsystem built in isolation meets, and the
first phase whose mistakes are user-visible rather than internal.

**Phase 5a ✅ — `check` end to end. Landed 2026-08-17.** Input resolution for
all three modes, both analysis layers, the union of their failure sets, gating,
exit 0/1/2, and `--format text|json`.

**Phase 5b ✅ — `doctor` and `init`. Landed 2026-08-17.** Ports of
`cli_doctor.py` (112 LOC) and `cli_init_hooks.py` (206 LOC) plus the `init-llm`
presets. Also closed the two gaps left open by 5a: the size ceiling now lives
on the rendered payload (`PAYLOAD_MAX_BYTES`), separate from the paths-mode
read guard (`READ_MAX_BYTES`), and `--format json`'s `unanalyzed` entries carry
a machine tag and HTTP status rather than only prose. See the CHANGELOG.

`drep init` writes **native git hooks**, not a `.pre-commit-config.yaml` entry.
2.0 is a single binary with no Python runtime, so requiring the `pre-commit`
framework to install a hook is a dependency the rewrite exists to shed. The two
hard-won parts of the 1.x installer carry over unchanged: the hooks directory
comes from `git rev-parse --git-common-dir` (in a linked worktree or a submodule
`.git` is a *file*, so `$REPO/.git/hooks` does not exist and the hook silently
never runs), and a `core.hooksPath` set anywhere means git ignores `.git/hooks`
entirely, so a chainer in that directory is what keeps a repo-local hook alive.

**Phase 5c ✅ — multi-provider LLM failover. Landed 2026-08-18.** See below;
deliberately last, because it changes the cache-key layer.

Decisions taken before 5a, so the spec does not have to re-litigate them:

- **The LLM is mandatory.** There is no `--no-llm` flag (Phase 1's exit
  criterion mentioned one; it was never added and is not wanted). A
  misconfigured or unreachable endpoint is a failure to surface, not a mode to
  fall back to.
- **`failed_files` becomes `BTreeMap<PathBuf, FailureReason>`.** A bare path set
  cannot tell a dead endpoint from a rate limit from a truncated response, and
  `analyze_file` currently discards the detail at `Err(_)` — so a 429 cannot be
  reported today even though `LlmError` carries it. Keep the status code
  structured (`Transport { status: Option<u16>, message: String }`) rather than
  only as a string, because 5c's failover needs to branch on it.
- **Tool-level `Unavailable` maps to file-level failure.** `ToolOutcome` is
  per-tool; `failed_files` is per-file. If ruff cannot execute, every Python
  file it would have checked is unanalyzed. This join is where exit 2 either
  works or silently does not.
- **The two failure sets union, never sum.** Both layers cover the same files.
- **A file too large for whole-file mode is `failed_files`, not skipped.** 1.x
  returned `[]` for anything over 32k chars, which under this contract is the
  banned move: a file we declined to analyze is not clean.
- **Progress output is display-only.** LLM calls take seconds and a silent hook
  reads as hung, but failures never travel through that channel.

Also due here, both deferred from Phase 3: a `git_ref` beginning with `--`
reaches git as a flag (now one site, `since_diff`), and `which_first` checks
`is_file()` rather than executability - which matters precisely because
`doctor` is the command that displays tool status.

**Decisions taken in 5b that later phases inherit:**

- **`drep check` accepts `--tip`, and the pre-push hook uses it.** `--diff
  <base>` means `git diff <base>...HEAD`, but git can push a ref that is not
  the checked-out one, so a hook that omits the tip reviews the wrong branch
  and lets the pushed one through unseen. Any future caller diffing on behalf
  of something other than the working checkout must name the tip.
- **A base search in a hook is bounded.** The "branch is new upstream" fallback
  stops at `<remote>/HEAD`, `<remote>/main`, `<remote>/master` or 50 commits.
  Falling back to the root commit sends an entire history to a reasoning model.
- **`config::env_var_refs_in` is the only definition of "a `${VAR}`
  reference".** `doctor` had its own, narrower one, which meant it reported a
  config as fine that `check` refused to load.

### Phase 5c ✅ — multi-provider failover (a deliberate reversal)

Landed 2026-08-18. `src/llm/chain.rs` owns the loop; the analyzer calls it
instead of `LlmClient` directly.

This rewrite dropped the circuit breaker and the rate limiter as server-shaped
complexity, and failover is the same category. It was taken anyway for a
specific failure that happens in practice: a local endpoint that is off blocks
every commit, because exit 2 is a hard stop by design. The reversal is recorded
here rather than slipped in, so the next person weighing "should drep grow
resilience machinery" sees the bar it had to clear.

#### The policy, as built

- **Only transport failures fail over** — status-less failures (timeout,
  refused connection, empty body) and the retryable statuses 408, 429 and 5xx.
  A 401 or 403 stops the chain: that is misconfiguration, and falling back
  masks it. Truncation is excluded — a different model might not truncate, but
  it doubles cost exactly when responses are longest.
- **Whether a failure is *remembered* is a separate question from whether it
  advances the chain**, and the sticky set is wider. `is_sticky` covers every
  endpoint-level failure including a 401: a stale key answers the same way for
  every file, and forty-nine TLS handshakes to be told so again is pure
  wall-clock on a gate that will exit 2 regardless. The remembered reason is
  replayed through `should_failover`, so a demoted 401 still stops the chain —
  otherwise every file after the first would skip the head and be served
  happily by the fallback, which is the exact masking the 401 rule forbids.
- **An empty response fails over.** It reaches the chain as
  `Transport { status: None }` only after the SDK has already retried it, and a
  provider that keeps answering with nothing is as unusable as one that is
  down. It also produced zero output tokens, so the attempts it burned cost
  almost nothing. This is the flakiness that failed 7 of 49 files on drep's own
  first gated push.
- **`enabled` is an opt-out, defaulting to `true`.** `Config::providers()` is
  the chain: enabled entries, in file order. A disabled *head* falls through to
  the entry below it rather than producing `NotConfigured`, which is what
  parking the local model was always meant to do. A config where every entry is
  disabled is rejected by `config::load` (`NoEnabledProviders`), the same rule
  as an empty list. The default flipped from `false` because an opt-out that
  defaults to "out" made declaring a provider do nothing until you also enabled
  it — a user adding a fallback by copying the first block, minus its `enabled`
  line, got a silently inert entry and no failover.
- **Demotion is sticky for the run.** The first file that fails over marks that
  provider down; later files skip it. The failure classes that fail over are
  endpoint-level, not file-level, so with a dead endpoint and forty-nine files,
  per-file retry pays the SDK's full backoff schedule forty-nine times for a
  verdict already known. The reason a provider went down is kept, not just the
  fact, because one transient 500 diverting the rest of a run to a paid
  endpoint has to be visible.
- **The demotion check runs twice: before the limiter and after it.** Files are
  analyzed concurrently, so the check before the limiter only stops files that
  had not started. Everything already queued passed it before the first failure
  landed. Without the second look, sticky demotion saves nothing in the one
  case it exists for; with it, the waste is bounded by `max_concurrent` rather
  than by the file count.
- **Each provider carries its own limiter.** `max_concurrent` is a per-`[[llm]]`
  field, and the slot represents in-flight work against one endpoint. A single
  shared limiter would apply the local model's generous budget to a
  rate-limited cloud endpoint.

#### The cache key moves with the provider

The key is computed from the provider's **endpoint and** model, inside the
loop, once per provider tried, and `Served::key` names whoever answered. Keying
the head and letting the fallback serve would file that answer under a key it
did not come from, and a later run with the head restored would get a hit that
never came from the head. Pinned at two levels: `llm/chain/tests/cache_key.rs`
asserts on the key itself and on a second run with the head restored, and
`analysis/tests/code_quality_failover.rs` asserts the actual `cache.put`
landed under the fallback's key and not the head's.

**The endpoint was missing from the key on the first cut, and every one of
those tests passed.** They all used distinct model names, so the key differed
for the wrong reason. Two providers running the *same* model at different
endpoints — one open model served locally and from a cloud provider, which is
the canonical failover pair — collided, and the fallback's answer was served to
the restored head. Composition now lives on `Provider::cache_key`, so a test
cannot spell the key out a different way than production does; spelling it out
by hand is what made the tests agree with the bug. Found by drep's own pre-push
gate reviewing this phase.

#### Reporting

- **Text** prints nothing about providers on the happy path — a line on every
  commit is noise. A run that fell through prints every provider that served,
  with model, endpoint and file count, because a silent switch from a local
  endpoint to a paid API is a cost surprise.
- **JSON** always carries a `providers` array, even for a single-provider run:
  a machine consumer has no noise problem, and an always-present field is what
  distinguishes "no failover" from "this build does not report it".
- **`FailureReason::ChainFailed`** carries every provider's reason, including
  the ones skipped as already-down, under the line "no LLM provider analyzed
  this file". Keeping only the last would hide a dead local endpoint behind the
  fallback's 401; keeping only the first would hide the broken fallback. It is
  phrased by what happened rather than by a count, because the list can be
  shorter than the chain — a 401 at the head stops it, and the providers below
  were deliberately never asked.
- **A one-provider *chain* collapses to that provider's own reason.** A
  single-provider config — what `drep init` writes — reports exactly what it
  reported before failover existed, JSON `kind` included. The trigger is the
  chain's length, not the attempt count: those differ precisely where the
  structure is worth most, because a 401 at the head of a two-provider chain
  yields one attempt, and collapsing it would discard the provider index and
  the model name just as the user asks why the fallback did not run.
- **`doctor` states the chain.** It bullets disabled entries `(disabled -
  skipped)` and numbers only the live ones, so its `1.` is the same provider a
  failure line calls `[1]` — numbering the file instead would make the two
  disagree the moment anything above was parked (`ConfigError` does number the
  file, and says "in file order" out loud for that reason). It then says which
  of three situations the config is in: no enabled provider at all, one
  provider and therefore no fallback, or N providers tried in order with the
  401/403 exception named. "It falls through" without "except on a 401" is the
  half that leads a user to expect a broken key to be routed around.
- **The served counts live on the chain, not on `AnalysisResult`.** The chain
  already knows who answered, is already shared for the process, and already
  holds per-provider interior-mutable state beside it. Carrying the counts out
  through the analysis result and rejoining them against the chain to recover
  each model and endpoint cost `AnalysisResult` a field whose merge rule was
  the one exception to its union-not-sum invariant.

#### What the tests could not do

Both defects found after 5b's suite went green were invisible to it for
structural reasons, and failover has the same shape of risk:

- wiremock returns exactly what it is told, so no test here shows a provider
  that *intermittently* returns nothing — which is how the retry-class bug
  survived, and which is the shape of every interesting failover case in
  production. The classification is pinned by the deterministic cases instead.
- A test that mounts a dead provider A and a healthy provider B proves the loop
  advances and nothing else. Three deliberate sabotages were run against the
  suite before it was trusted: keying on the head instead of the serving
  provider (3 of 4 `cache_key` tests failed), failing over on every status (2
  `failover` tests failed), and deleting demotion (all 4 `demotion` tests
  failed). A suite that survives those is worth having; one that only checks
  the returned value is not.

### Correction to Phase 3's retry classification (2026-08-18)

Phase 3 split LLM outcomes two ways — retry transport failures, never retry a
parse failure — and Phase 5c's push showed the split had one case too few.

The rule "a non-empty body that yields no JSON must never retry" was justified
as *"the same prompt truncates the same way"*. But truncation is
`Extracted::Truncated`, a **different branch**: brace-balancing recovers a
prefix and returns it. A body with *no JSON at all* did not truncate an answer,
it never produced one, and that does not repeat — three of four attempts at the
Phase 5c push died on it, a different file each time, and every failing file
analyzed cleanly when asked again on its own.

There are three outcomes, not two:

| response | classification | retried? | fails over? |
|---|---|---|---|
| empty | `Transport` | yes, by the SDK | yes |
| no JSON at all | `Unparseable` | yes, `NO_JSON_ATTEMPTS` | **no** |
| parsed after brace-balancing | `Truncated` | no | n/a |

The no-JSON retry lives in `complete_json`, deliberately outside the SDK's
retry layer. Handing it to the SDK by returning `Err` would retry correctly and
then surface as `Transport` once attempts ran out — which fails over to the next
provider *and* demotes this one for the whole run. A model that answered in
prose has told us nothing about the endpoint.

`LlmError::Unparseable` now carries a bounded, control-character-stripped
excerpt of the body. It was a constant string, so every occurrence of this
failure looked identical and there was no way to tell a refusal from a prose
preamble from reasoning that leaked into the content channel.

**Two things the SDK still hides, and they are why the excerpt exists.**
`open-agent-sdk`'s `StreamAccumulator` consumes `finish_reason` internally and
never surfaces it, so drep cannot distinguish `"stop"` from `"length"` — a
response cut off at the token cap looks identical to a complete one.
`OpenAIDelta` also deserializes only `role`/`content`/`tool_calls`, so the
`reasoning` and `reasoning_content` fields that DeepSeek and OpenRouter stream
are dropped as unknown fields. Both are fixable in the SDK (same author); until
then the excerpt is what turns the next occurrence into a diagnosis instead of
a guess.

### Phase 6 — `lint-docs`
Port the markdown checks including `_fence_mask`. No LLM, fast, runs on every
commit.

**Carries one gap from 5a/5b.** `files::is_scan_target` accepts `.md`, but no
language claims it, so `drep check README.md` reads the file, finds no
deterministic tool and no LLM language, and prints "No issues found." That is
the banned move — a file drep declined to analyze reported as clean — applied to
a path the user named explicitly. The same distinction `resolve_paths` already
draws for a non-existent argument applies here: a *walk* that finds no
analyzable file is legitimately empty, an explicitly-named one is not. Giving
`.md` a real home here is the fix; until then it is a known hole.

### Phase 7 — Distribution
`cargo-dist` 0.32 (repo active as of 2026-08-17) generates the release
workflow, multi-arch binaries, the shell installer, **and** the Homebrew
formula pushed to a `slb350/homebrew-tap` repo. Config:

```toml
[workspace.metadata.dist]
installers = ["shell", "homebrew"]
tap = "slb350/homebrew-tap"
publish-jobs = ["homebrew"]
targets = [
  "aarch64-apple-darwin", "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
]
```

Requires a `HOMEBREW_TAP_TOKEN` secret with `repo` scope. Also rewrite
`.pre-commit-hooks.yaml` to `language: rust` (or `language: system` against the
installed binary) — the current `language: python` entry dies with the package.

### Phase 8 — Delete Python
Remove `drep/`, `tests/`, `pyproject.toml`, `scripts/install.sh`. Rewrite
`README.md`, `CLAUDE.md`, and `docs/technical-design.md` for the new scope.
Yank nothing from PyPI; just stop publishing.

## Invariants that must survive the port

These were learned the hard way and are not obvious from reading the code.
Each needs a test in the Rust suite.

**Tool execution**
- Resolve **repo-local before PATH** — a project is checked by the version its
  own CI runs (`node_modules/.bin/eslint`, not the global one).
- Run a tool **only where the project configured it**. No eslint config means
  no eslint opinion, not a wall of default-preset complaints.
- `unavailable` is **not a pass**. A missing tool is a distinct outcome.
- Honour `diagnostics_stream`. `go vet` writes to **stderr**; reading stdout
  alone reports every Go file clean.
- **A tool that does not take file arguments is invoked bare, and its output
  narrowed afterwards** (`ToolSpec::accepts_files`). `cargo clippy` checks a
  *crate* and rejects a path outright, so appending files made every Rust run
  fail — the deterministic half for Rust did not run at all from Phase 1 until
  drep gating its own push exposed it. Narrowing matters as much as the
  invocation: a whole-crate run reports pre-existing issues in untouched code,
  which a commit gate cannot act on. **Stub-based `run_tool` tests cannot see
  this class of bug** — a stub accepts any argv. Only the real binary can say
  whether it accepts the one drep builds.
- Unparseable tool output is **`unavailable`, not zero findings**. Output we
  cannot read means we do not know whether the file is clean, and guessing
  "clean" is the failure the whole module exists to prevent. The one exception
  is line-oriented formats: `position` and `tsc` *skip* non-matching lines by
  design, because Go interleaves `# package/path` headers among its
  diagnostics. `cargo`'s NDJSON does not get that latitude — a line that is not
  JSON means we are not reading what we think we are.

**Gate semantics**
- Deterministic findings block; LLM findings inform. Split by **source**, not
  severity — severity thresholds over LLM output were never calibratable.
- Exit **2** for "could not analyze", **1** for "analyzed, found issues", **0**
  for clean. A gate that green-lights on an unreachable endpoint is worse than
  no gate.
- Analysis failures propagate to a `failed_files` set on the result. No blanket
  catch-all that returns an empty finding list. Failures never travel through
  the progress/display channel.
- Rank severity by **index into the ordering**, not a lookup with a default. An
  unrankable severity is a bug to surface, not a finding that silently passes.

**LLM client**
- `max_retries` is a **total attempt count with a floor of 1**. Config permits
  0, and a naive `0..max_retries` loop skips the request entirely and then
  reports a bogus "no exception captured".
- **An empty response body is a transport failure and retries; a non-empty
  unparseable one does not.** Both were one non-retrying `Ok(None)` until
  drep's own gated push failed 7 of 49 files and an immediate re-run of one
  succeeded — so "the model returned nothing" is provider flakiness, not a
  property of the prompt. Zero output tokens cost nothing to produce, so
  retrying is nearly free; re-sending a prompt whose answer was prose buys the
  same prose for a full reasoning call. Assert the **request count**, not just
  the classification: labelling the empty body correctly while still refusing
  to retry passes every other check.

**Diff parsing and the LLM payload**
- The file a hunk belongs to comes from the **`+++ b/<path>` line**, never from
  `diff --git a/… b/…`. The git header carries two paths on one line with no
  unambiguous separator, so any "find `b/`" rule captures the wrong span for a
  repository path that itself contains `b/` (`src/b/mod.rs`). The Python
  `diff_parser.py` had exactly this bug.
- **Inside a hunk body, the first byte alone decides the line kind.** Do not
  also skip lines starting with `---` or `+++`: those headers appear only before
  the first `@@` of a file, and a removed source line beginning with `--`
  arrives as `---…`. Skipping it silently drops real removed code.
- **The payload states each line's true new-file line number**; the model is
  never asked to derive one from an `@@` header. Removed lines render with a
  blank number field because they have no line in the new file. A finding
  outside `Payload::valid_lines` is about code that was never sent, and is
  dropped rather than clamped onto whatever line is nearest.

**Markdown**
- One `fence_mask` answers "is this line inside a code fence", consulted by
  every check. Per-check `in_fence` toggles are how `#!/bin/bash` inside a bash
  sample got reported as a malformed heading.

**File walking**
- Prune vendored directories **during** the walk. The Python version used
  `os.walk` with in-place pruning specifically because `rglob` stats every entry
  under `venv/` before discarding it. The `ignore` crate does this correctly by
  default.
- Deduplicate CLI path arguments. `drep check a.py .` must not pay two LLM
  round-trips for one file.

## Deliberate behavior changes

Not ports — decisions.

**Diff-based, not whole-file.** `code_quality/analyzer.py::analyze_file` sends
entire file contents (capped at 32k chars). 2.0 sends changed hunks plus their
enclosing context. Cheaper per run and correctly scoped to a commit gate. The
cache key changes accordingly: hunk content, not file content.

**Docstring generation dropped; `ast` leaves the codebase.** The docstring
analyzer needed `ast.parse` because it *generated* docstrings and had to find
insertion points. Reporting a missing or poor docstring needs no parser — this
is already proven by `code_quality`, which has language-aware opinions with
zero parsing. Consequence: no tree-sitter, no Python interpreter, and the check
generalizes to all five languages instead of Python only.

**Config is TOML.** No users to migrate, and TOML is native to the ecosystem
and to `cargo-dist`. `drep init` writes it.

**No database.** Both tables served the platform features. Diffing against git
replaces the incremental-scan SHA.

**Simplified concurrency.** The rate limiter and circuit breaker were built for
a server scanning whole repositories against a shared endpoint. A local binary
reviewing a handful of hunks against your own LM Studio keeps concurrency
capping and drops the global token-per-minute budget and half-open probe
machinery. Revisit only if a real workload demands it.

## Done criteria

- `brew install slb350/tap/drep` yields a working binary on Apple Silicon,
  Intel macOS, and x86_64/aarch64 Linux.
- `drep check --staged` gates a commit in a Python, TypeScript, Go, and Rust
  fixture repository, with no Python installed in the Go and Rust cases.
- Exit codes 0/1/2 are correct, including the unreachable-endpoint case.
- `cargo clippy -- -D warnings` and `cargo fmt --check` clean.
- `drep check` runs green over drep's own source, with **zero** files
  unanalyzed — the deterministic tools actually executing, not merely resolving.
- Python removed from the repository.
