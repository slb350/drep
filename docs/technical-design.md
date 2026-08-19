# Technical design: drep 2.0

drep is one binary. It reads files, runs the tools the repository configures,
asks a model about the code that changed, and exits 0, 1 or 2. There is no
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
| `llm/client/` | a thin layer over `open-agent-sdk` |
| `llm/cache.rs` | content-addressed response cache |
| `llm/concurrency.rs` | the permit limiter |
| `llm/json_parsing.rs` | tolerant extraction of JSON from a model response |
| `analysis/payload.rs` | rendering a file or hunk set into the prompt payload |
| `analysis/code_quality.rs` | the semantic pass |
| `analysis/findings.rs` | `Severity` and its ordering |
| `docs/` | the markdown checks: `fence`, `lines`, `links`, `blocks` |
| `config.rs` | `drep.toml`: parse, `${VAR}` expansion, validation |
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
- A tool runs only where the project configured it, decided by
  `ToolSpec::config_files`.
- `unavailable` is not a pass. `ToolSpec::diagnostics_stream` exists because
  `go vet` writes diagnostics to stderr, and reading stdout alone reported
  every Go file clean.

A whole-project tool is invoked bare and its findings narrowed to the files
being checked: `cargo clippy` takes no file arguments (`accepts_files = false`)
and rejects a path outright.

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
`llm/chain.rs` tries them in turn.

Two questions, deliberately answered by different predicates:

- `should_failover` decides whether the chain advances. Status-less failures
  (timeout, refused connection, empty body) and 408/429/5xx advance. A 401/403
  stops it, because that is misconfiguration and falling back would mask it. A
  non-empty unparseable body stops it, because it is deterministic.
- `is_sticky` decides whether the failure is remembered for the run. It is
  `should_failover(err) || is_auth_failure(err)`, never "every transport
  failure": remembering a failure that does not fail over stops the chain for
  every later file.

Demotion is sticky for the run and re-checked after the concurrency limiter,
because files run concurrently and a check taken only before acquiring a permit
stops nothing that had already started.

A one-provider chain collapses to that provider's own `FailureReason`;
`ChainFailed` appears only for a chain of two or more, so the config
`drep init` writes reports exactly what it did before failover existed.

### Responses

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
characters of the key. The key is blake3 over five length-prefixed inputs and
**includes the endpoint**, not just the model, computed inside the failover
loop through `Provider::cache_key`. One open model served both locally and from
a cloud provider is the canonical failover pair and both name it identically.
Defaults: 30-day TTL, 1 GiB.

## Failure contract

The analyzers propagate transport and parse failures rather than swallowing
them. A file that could not be analyzed is counted, reported in its own block,
and makes the run exit **2**. Exit 1 means analysis ran and found something
blocking. `--format json` carries `unanalyzed` alongside `findings` so a
consumer can tell a clean run from one that never happened.

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
config and must not be hand-edited.

CI (`.github/workflows/rust.yml`) runs fmt and clippy once, the test suite on
Linux and macOS, an MSRV check at 1.88, and a full `cargo mutants` sweep. The
sweep is local to the runner because a GitHub runner cannot reach the LAN host
the pre-commit hook offloads to.
