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
tests/rust/          # new (integration tests)
drep/                # Python, reference only, deleted in Phase 8
tests/               # Python, reference only, deleted in Phase 8
pyproject.toml       # deleted in Phase 8
```

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
| `src/files.rs` | `core/file_targets.py` | Uses the `ignore` crate for the walk |
| `src/diff/parser.rs` | `pr_review/diff_parser.py` | Now the primary input path |
| `src/diff/git.rs` | `llm/git_utils.py` | Shells out to `git`; no libgit2 |
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
open-agent-sdk = { git = "https://github.com/slb350/open-agent-sdk-rust", tag = "v0.6.9" }
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
resolution, subprocess execution, and the three output parsers (json, tsc,
lines). Ends with `drep check --no-llm PATHS` producing blocking findings from
real ruff/eslint/gofmt runs against fixture repos.

### Phase 2 — File targeting and diff
`src/files.rs` on the `ignore` crate (replaces the hand-rolled walk + prune;
gitignore-awareness comes free). `src/diff/` for `--staged` and `--diff <ref>`,
shelling out to `git`. Ends with correct file sets for all three input modes,
deduped.

### Phase 3 — LLM layer
Wire `open-agent-sdk`. Port the cache (content-addressed on prompt + content +
model + temperature), tolerant JSON parsing, and a simplified concurrency
limiter. Test against `wiremock`, not a live endpoint.

### Phase 4 — Analysis and exit codes
`code_quality.rs` sending **diff hunks with enclosing context**, `findings.rs`
with the severity vocabulary, and the failure contract: unanalyzed files are
counted and surfaced, never reported clean.

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
