# Technical design: drep 2.0

drep is one binary. It reads files, runs the tools the repository configures,
asks a model about the code that changed, and exits 0, 1, 2 or 3. There is no
server, no database, no platform client and no background work.

This document is the structure. `CLAUDE.md` carries the invariants, each with
the defect that produced it; where the two overlap, this file says what the
shape is and `CLAUDE.md` says why it cannot change.

## The pipeline

```
arguments / --staged / --diff <ref>
        |
   files::expand_named  ->  Expansion { targets, rejected }
        |
   Work items  (path + content, or path alone when the file is too large)
        |
        +--> deterministic: languages::runner  ->  findings that gate
        |
        +--> semantic:      analysis::code_quality -> findings that inform
        |
   CheckOutcome { findings, unanalyzed }  ->  gate  ->  render  ->  exit code
```

`check` and `lint-docs` share the last two steps and nothing before them. They
own disjoint file classes: `files::is_scan_target` is registered-language
source, `files::is_markdown` is markdown, and nothing satisfies both.

## Modules

| Module | Responsibility |
|---|---|
| `main.rs` | clap entry point; turns every error into an `ExitCode` |
| `cli/check/` | `input` (what to analyze), `deterministic` (tool half), `render`, exit codes |
| `cli/lint_docs/` | markdown command, `--staged`, `--fail-on` |
| `cli/doctor.rs` | what languages and tools are visible here, and which providers |
| `cli/init/` | `drep.toml` and native git hooks; `presets`, `config_file`, `hooks` |
| `cli/render.rs` | the finding line and the "could not be analyzed" block, shared |
| `languages/spec.rs` | `ToolSpec`, `LanguageSupport`, the registry |
| `languages/definitions.rs` | the language table: extensions, tools, conventions |
| `languages/runner/` | tool resolution, execution, and the output parsers |
| `files/` | file-class predicates, the walk, and CLI path expansion |
| `diff/` | `--staged` and `--diff <ref>`, by shelling out to git |
| `llm/chain.rs` | the provider chain: failover, demotion, per-provider cache key |
| `llm/backend.rs` | the HTTP/Codex backend boundary used by each provider |
| `llm/client/` | a thin layer over `open-agent-sdk` |
| `llm/codex/` | isolated Codex command, diagnostic, process bounds and JSONL parser |
| `llm/cache.rs` | content-addressed response cache |
| `llm/concurrency.rs` | the permit limiter |
| `llm/json_parsing.rs` | tolerant extraction of JSON from a model response |
| `llm/models.rs` | asking an endpoint which models it serves |
| `llm/quirks.rs` | the cached model-quirks registry, distilled from models.dev |
| `analysis/payload.rs` | rendering a file or hunk set into the prompt payload |
| `analysis/code_quality.rs` | the semantic pass |
| `analysis/findings.rs` | `Severity` and its ordering |
| `docs/` | the markdown checks: `fence`, `lines`, `links`, `blocks` |
| `config.rs` | `drep.toml`: parse, `${VAR}` expansion, validation |
| `auth.rs` | the per-machine credential store, keyed by endpoint |
| `text.rs` | `excerpt`, the only bounding of text drep did not write |

## Two layers, split by source

`languages/` is the only place a language is named. A `LanguageSupport` carries
its extensions, its deterministic tools and the conventions its prompt names;
`registry::detect(path)` answers "which language" everywhere. There is no
`if python` branch in the tree, so adding a language is an entry in
`definitions.rs`.

**Deterministic** findings come from the project's own tools and always gate.
Three rules govern them:

- Repository-local resolution before PATH, so a project is checked by the
  version its CI runs.
- Each file is assigned to its nearest configured ancestor, decided by
  `ToolSpec::config_files`; batches are per tool and workspace. Local
  executable resolution walks from that workspace to the repository root, so
  hoisted dependencies remain available. Ordinary tool processes are capped
  at four concurrent invocations; repository-wide clippy tasks are serialized
  so workspace fan-out does not create build-lock contention itself.
- `unavailable` is not a pass. `ToolSpec::diagnostics_stream` exists because
  `go vet` writes diagnostics to stderr, and reading stdout alone reported
  every Go file clean.

A whole-project tool is invoked bare and its findings narrowed to the files
being checked: `cargo clippy` rejects path arguments, and passing paths to
`tsc` makes it ignore `tsconfig.json`, so both use `accepts_files = false`.
Clippy's process ceiling is 30 minutes because Cargo's build-lock wait happens
inside that process; other tools retain the two-minute default.

**Semantic** findings come from the model and inform unless `--fail-on` opts
them into gating. The model parses nothing, which is why multi-language support
needed no grammars. Severity thresholds over LLM output were never a usable
gate; splitting by source is what makes one calibratable.

## Input and diff modes

`files::expand_named` is the single answer to "what did the user ask for, and
what could I not do with it". It returns `Expansion { targets, rejected }`,
resolves an empty argument list to the repository root, and takes one
`fs::metadata` per named path. A file the running command's predicate rejects
is `FailureReason::Unsupported`, carrying the extension and what to run
instead; a walk that finds nothing analyzable is legitimately empty.

Every `diff` query takes the file-class predicate as a parameter:
`staged_files`, `changed_since`, `staged_hunks`, `hunks_since`, `hunks_between`.
`check` passes `is_scan_target`, `lint-docs --staged` passes `is_markdown`.

Two size ceilings, deliberately separate. `analysis::payload::PAYLOAD_MAX_BYTES`
(256 KiB) is checked against the rendered payload, so it holds for every input
mode and is the authority on "too large to analyze".
`cli::check::input::READ_MAX_BYTES` (8 MiB) only stops `read_to_string` pulling
a pathological file into memory. A file too large for the model is still
linted, through `Work::lint_only`.

## The LLM layer

### The payload

`analysis/payload.rs` renders each line as `{marker}{n:>6} | `, with a blank
number for removed lines. The model is never asked to derive a line number from
an `@@` header, and a finding on a line outside `Payload::valid_lines` is
dropped rather than clamped to the nearest one.

### The chain

`[[llm]]` is an array of tables and the order is the failover chain.
`Config::providers()` returns the enabled entries in file order and
`llm/chain.rs` tries them in turn. Each `Provider` owns a `ProviderBackend`:
`Http(LlmClient)` or `Codex(CodexClient)`. The chain, cache and analyzer depend
on that boundary rather than on an HTTP endpoint.

Two questions, deliberately answered by different predicates:

- `should_failover` decides whether the chain advances. Status-less failures
  (timeout, refused connection, empty body) and 408/429/5xx advance. A 401/403
  stops it, because that is misconfiguration and falling back would mask it. A
  non-empty unparseable body advances after three response attempts, because a
  fallback can salvage model-side garbling.
- `is_sticky` decides whether the failure is remembered for the run. It is
  the endpoint-level subset of failover errors plus authentication/contract
  failures. An unparseable answer advances for this file but is not sticky,
  because another file from the same provider may parse normally.

Backend failures carry a typed kind. Authentication and contract failures stop
and are sticky; a request-shaped failure stops without poisoning later files;
an explicitly classified usage-limit failure can fail over and is sticky.
Current Codex JSONL emits only a human message for terminal failures, so an
unknown nonzero exit stays `UnknownExit` and is never classified by matching
prose.

Demotion is sticky for the run and re-checked after the concurrency limiter,
because files run concurrently and a check taken only before acquiring a permit
stops nothing that had already started.

A one-provider chain collapses to that provider's own `FailureReason`;
`ChainFailed` appears only for a chain of two or more, so the config
`drep init` writes reports exactly what it did before failover existed.

### Responses

The response schema includes `compile_failure`, an explicit statement that the
finding claims the code cannot compile. Successful clippy, tsc, or go-vet
outcomes are carried back to the orchestrator per file; only a matching LLM
compile-failure claim is suppressed. Semantic findings are unchanged.

Each accepted LLM finding is fingerprinted from its file, category and nearby
source. `.drep/acknowledgements.toml` stores rejected fingerprints. Checks load
the file read-only and filter matches; `drep acknowledge` is the only writer,
publishing it atomically. Because line numbers are excluded from the hash, a
pure line shift does not resurrect a finding, while a local source edit does.

The HTTP backend uses `open-agent-sdk`. The subscription backend invokes
`codex exec --json` with ChatGPT authentication forced. It runs in an empty
temporary working directory with an allowlisted environment, ephemeral state,
read-only sandboxing, approval disabled, user/project configuration ignored,
and every available tool surface disabled. The payload is written to stdin;
the existing review contract is supplied as a replacement instructions file;
a strict output schema constrains the final agent message.

The subprocess deadline covers stdin, stdout, stderr and exit. Output is
bounded, the child is killed and reaped on timeout, and stderr is retained only
as a short sanitized excerpt. The JSONL parser accepts lifecycle/progress,
reasoning, todo-list and one final agent message followed by `turn.completed`.
Command, file-change, MCP, web-search and subagent events are contract
violations rather than ignorable progress. A terminal error event is surfaced
as a bounded `UnknownExit` diagnostic without classifying its human message.

`finish_reason` decides whether asking again can help. `Length` and
`ContentFilter` are request-shaped: the same request hits the same cap, so they
end the attempt as `ModelStopped`, never fail over and never demote. Everything
else stays retryable, including `Unspecified`, which several OpenAI-compatible
servers always return.

Three response outcomes, three rules:

| Outcome | Class | Retried? | Fails over? |
|---|---|---|---|
| Empty body | transport | yes | yes |
| Parsed only after brace-balancing (`Truncated`) | deterministic | no | no |
| No JSON at all | `Unparseable` | up to `NO_JSON_ATTEMPTS` | no |

The no-JSON retry lives in `complete_json`, not in the SDK's retry layer:
returning `Err` there would surface it as `Transport` once attempts ran out,
which would fail over and demote a provider over a prose preamble.

### Cache

A directory of one JSON file per entry under
`directories::ProjectDirs::cache_dir()`, sharded by the first two hex
characters of the key. The key is blake3 over six length-prefixed inputs and
includes a backend identity, not just the model, computed inside the failover
loop through `Provider::cache_key`. HTTP identity includes endpoint and wire
protocol. Codex identity includes ChatGPT auth mode, CLI version and reasoning
effort. One model served by the OpenAI API and by a ChatGPT subscription must
never share an entry. Defaults: 30-day TTL, 256 MiB.

Writes go through a uniquely named temporary file in the destination shard and
atomically replace the canonical entry. A concurrent reader therefore sees the
old complete JSON or the new complete JSON, never an in-progress write, and a
destination symlink is replaced rather than followed.

`--cache-only` walks the provider chain's cache identities without contacting
a backend. A miss is distinct from failed analysis and exits 3. `--push-gate`
uses that mode first: a warm diff passes immediately; a cold diff is reviewed
and cached in the foreground, then exits 3 so Git discards the remote
connection that sat idle during review. The next `git push` reconnects and is a
fast cache-only verdict. A failed warm or blocking finding retains exit 2 or 1
and never prints the reconnect instruction.

## Failure contract

The analyzers propagate transport and parse failures rather than swallowing
them. A file that could not be analyzed is counted, reported in its own block,
and makes the run exit **2**. Exit 1 means analysis ran and found something
blocking. `--format json` carries `unanalyzed` alongside `findings` so a
consumer can tell a clean run from one that never happened.

Exit 3 is the successful pre-push reconnect handshake, not a failure to
analyze: the missing review completed and is now cached, but the current Git
transport is intentionally abandoned before it resumes after a long idle.
Generated hooks map every unknown nonzero drep status to exit 2, so a crash,
signal, or future exit code cannot silently let a push through.

`LintOutcome` and `CheckOutcome` carry the gate's decision rather than letting
the renderer re-derive it from the findings and the threshold. The threshold
governs findings, not failures: `lint-docs` exits 2 for a file it could not
read whatever `--fail-on` says.

## Markdown

Ten checks, one pass, no network and no config file. `docs::fence::Fences` is
derived once per file and is the single answer to "is this line inside a code
fence"; a check that tracks fence state itself is the bug. Which checks consult
it is decided by one question: would this check's advice be wrong inside a
fence? Headings, `long_line` and the link checks, yes. `tab_character`, yes,
because "replace tabs with spaces" would break a Makefile sample.
`trailing_whitespace`, no.

Severity answers whether the finding changes how the document renders, which is
what keeps `--fail-on` calibratable. `unclosed_code_fence` alone is `error`.

`lint-docs` touches `docs` and `files` and nothing else: no config file, no
provider chain, no cache. The 1.x equivalent paid 190 ms of sqlalchemy and
GitPython on every commit.

## Configuration

`drep.toml`, discovered under the repository root. `api_key` names an
environment variable rather than holding a secret, and `config::env_var_refs_in`
is the single definition of a `${VAR}` reference, shared by the substituter and
by `doctor`.

`backend` defaults to `http`, preserving every pre-existing file. HTTP entries
accept `endpoint`, `api_key`, `protocol`, `temperature`, `max_tokens` and
`max_retries`; they reject the Codex-only `reasoning_effort`. A `codex` entry
requires `model`, accepts an optional `reasoning_effort`, and rejects every
HTTP-only field, so a subscription selection cannot silently become API
traffic. It requires the separately installed Codex CLI and ChatGPT-managed
login; drep's endpoint-keyed auth store is not involved.

Validation is deliberately strict at load, because each of these can only fail
later and less legibly: a file declaring no providers, or none enabled, is
rejected; `max_concurrent = 0` is rejected, since a semaphore with no permits
would hang with no message. Disabled entries are inert - `${VAR}` expansion and
field validation both skip them - so parking a cloud provider does not require
its key to be set. `LlmConfig` hand-writes `Debug` to redact `api_key`.

## Distribution

`dist-workspace.toml` drives cargo-dist. Four targets, each built on a runner
of its own architecture, plus a shell installer and a Homebrew formula pushed
to `slb350/homebrew-tap`. `.github/workflows/release.yml` is generated from that
config, with one tested compatibility override pinning the artifact actions to
the v4 protocol supported by family Gitea. `allow-dirty = ["ci"]` declares that
override to cargo-dist. The same workflow creates the GitHub release and
publishes Homebrew; crates.io is a separate `cargo publish --locked` step
because cargo-dist does not publish Rust crates.

CI (`.github/workflows/rust.yml`) runs fmt and clippy once, an MSRV check at
1.88, and a full `cargo mutants` sweep. GitHub runs the test suite on Linux and
macOS; the family Gitea instance runs Linux only because it has no macOS runner.
Gitea 1.25.1 matches a runner before evaluating a job guard, so the guarded
macOS job resolves to `ubuntu-latest` on Gitea, where Strix can claim and skip
it, while GitHub resolves the same expression to `macos-latest`.
The hosted mutation job pins `cargo-mutants` 27.1.0 inside `node:22-trixie` so
its glibc 2.39 requirement does not depend on the family runner's older default
job image. The sweep is local to the runner because a GitHub runner cannot reach
the LAN host the pre-commit hook offloads to.
