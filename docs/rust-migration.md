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
| `src/cli/check.rs` | `cli_workflows.py::_run_check` | Exit codes 0/1/2 |
| `src/cli/lint_docs.rs` | `cli.py::lint_docs` | |
| `src/cli/doctor.rs` | `cli_doctor.py` | Language/tool detection |
| `src/cli/init.rs` | `cli_init_hooks.py` + init-llm | Writes hooks + config |
| `src/languages/spec.rs` | `languages/base.py` | `ToolSpec`, `LanguageSupport`, registry |
| `src/languages/definitions.rs` | `languages/definitions.py` | The language table |
| `src/languages/runner.rs` | `languages/runner.py` | Tool resolution + execution |
| `src/languages/parsers/` | `languages/runner.py` parsers | json / tsc / lines output formats |
| `src/files/mod.rs` | `core/file_targets.py` | `ignore` crate for the walk |
| `src/diff/mod.rs` | `pr_review/diff_parser.py` + `llm/git_utils.py` | `--staged` and `--diff <ref>`; shells out to git |
| `src/llm/client.rs` | `llm/client.py` | Thin over `open-agent-sdk` |
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

### Phase 4 — Analysis and exit codes
Split into 4a and 4b: together they exceed the size where a single delegated
handoff stays reliable, and 4a is fully offline while 4b is not.

**Phase 4a ✅ — hunks and payload. Landed 2026-08-17.** `src/diff/hunks.rs`
(parser), `staged_hunks`/`hunks_since` in `src/diff/mod.rs` at
`--unified=20`, and `src/analysis/payload.rs`. See the CHANGELOG for the full
note. `findings.rs` already carried `Severity` and `Finding` from Phase 0, so
the phase list's mention of it was stale.

**Phase 4b — analysis and the failure contract.** `code_quality.rs`: prompt
assembly from `LanguageSupport::conventions`, the LLM call through
`LlmClient`/`Cache`/`Limiter`, mapping the response to `Finding`s, and
`AnalysisResult { findings, failed_files }`. Severity mapping is
critical|high → error, medium → warning, low|info → info.

Two rules for 4b that are decisions, not ports:

- **`Extracted::Truncated` records the file in `failed_files` *and* returns its
  partial findings.** Unconditionally — the analysis layer never consults
  `--fail-on`. Phase 5's CLI decides what a failed file maps to. Conditioning it
  on the flag would make `failed_files` flag-dependent, so the JSON `unanalyzed`
  field would mean different things per invocation.
- **A finding whose line is not in `Payload::valid_lines` is dropped, not
  clamped** — it is about code the model was never shown. Dropped findings are
  counted so the drop is observable rather than silent. A *malformed* finding
  record (unknown severity, missing field) is different: it makes the file
  unanalyzed, because we cannot know what it said.

### Phase 5 — CLI assembly
`check` end to end: deterministic findings block, LLM findings inform unless
`--fail-on`, `--format text|json`, exit 0/1/2. Then `doctor` and `init`.

### Phase 6 — `lint-docs`
Port the markdown checks including `_fence_mask`. No LLM, fast, runs on every
commit.

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
- Python removed from the repository.
