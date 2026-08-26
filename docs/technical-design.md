# Technical design: drep

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
| `cli/check/` | `input`, deterministic tools, `review_budget`, rendering and exit codes |
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
| `config/env.rs` | `${VAR}` substitution and the one definition of a reference |
| `config/site.rs` | the machine-level policy file, and the concurrency ceiling |
| `auth.rs` | the per-machine credential store, keyed by endpoint |
| `auth/command.rs` | resolving a provider credential by running a configured argv |
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

The system prompt defines a high-signal merge-review threshold: a finding must
be concrete, reachable, materially consequential and worth fixing before
merge. It excludes speculative hardening, implausible extreme edge cases, nits,
cleanup and optional refactors.

Authoritative staged, diff, pre-commit-push and bare push-gate checks enforce a
three-round semantic-remediation budget by default. The accounting is
two-phase: an atomic pending slot is reserved before a cold provider request,
then retained only if the fresh result still has an actionable finding after
compile-claim suppression and acknowledgements. Clean results and pure
analysis failures refund the slot. Mixed findings and failures retain it.
Cached verdicts and deterministic tools remain available at the limit; a cold
semantic miss becomes `ReviewLimit` and exits 2 without contacting a provider.
Once reserved, the selected misses bypass cache so a concurrently published
cache entry cannot be counted as this process's fresh response.

The slots live under the current worktree's Git directory, partitioned by
branch identity. A worktree-wide advisory lock and fixed, atomically-created
filenames prevent concurrent checks from oversubscribing the configured limit.
Each pending lease carries an owner token; commit and refund verify it while
holding the same lock, so an expired owner cannot alter its successor's slot. A
clean complete diff, pre-commit-push or bare push-gate check removes committed
slots while preserving another process's pending reservation. Empty branch
state is removed. Pending or incomplete slots older than seven days are
recoverable after a killed process. `max_review_rounds` defaults to 3;
`--max-review-rounds N` raises it for one run and `--unlimited-reviews` is the
explicit escape hatch.

Claim, commit and reset writes fail closed. They are authoritative quota state,
not disposable response-cache maintenance: reporting a clean reset that could
not be persisted would make the rendered cycle state disagree with the next
invocation. Cache eviction remains best-effort because losing cached responses
changes only latency, not authorization.

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

The published pre-commit pre-push hook uses `--pre-commit-push` with filename
passing disabled. That adapter reads pre-commit's FROM/TO ref environment and
feeds it to `hunks_between`, so the published hook and the native hook both
review a pushed diff. When pre-commit deliberately omits refs for a new branch
whose history reaches a root commit, the adapter preserves its all-files
decision. Missing, partial or non-UTF-8 hook context fails instead of silently
falling back to paths mode.

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
`ChainFailed` appears only for a chain of two or more, keeping single-provider
errors direct while preserving per-provider detail for a failed chain.

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
| No JSON at all | `Unparseable` | up to `NO_JSON_ATTEMPTS` | yes |

The no-JSON retry lives in `complete_json`, not in the SDK's retry layer:
returning `Err` there would surface it as `Transport` once attempts ran out,
which would misclassify a model-response problem as transport. A final
`Unparseable` may fail over for that file but never demotes the provider.

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
provider chain, and no cache.

## Configuration

`drep.toml`, discovered under the repository root. `api_key` names an
environment variable rather than holding a secret, and `config::env_var_refs_in`
is the single definition of a `${VAR}` reference, shared by the substituter and
by `doctor`. The top-level `max_review_rounds` defaults to 3 and rejects zero.

A provider's credential is resolved in one pass, in `auth::resolve`, in this order: an explicit `api_key`, then `api_key_command`, then the endpoint-keyed store, then nothing — which `LlmClient::new` turns into `not-needed`. `auth::source_of` is the only statement of that order, and `doctor` calls it rather than restating it. `api_key_command` is an argv run with no shell, whose whole trimmed stdout is the credential; setting it alongside `api_key` is rejected at load, beside the unknown backend and the misspelled protocol, because two answers to one question is not a precedence puzzle. A Codex entry rejects it with the other HTTP-only fields.

It resolves in that one pass, so each entry's command runs exactly once per process: a short-lived credential re-minted per file fails per file, which the chain's demotion logic reads as an endpoint problem. There is no disk cache and no TTL behind it — drep is a short-lived process, so a credential on disk buys nothing and adds a file worth stealing.

A failing command is fatal there, before the chain exists, which is what makes it structurally unable to fail over. That is the same rule a 401 follows: routing around a broken credential path is what hides it. The diagnostic names the program and the exit status and nothing else, because a misconfigured helper can print the token to either stream and an error message is the one place it would escape. `KeyCommandError` carries no captured output, which is what keeps that true of `{:?}` as well as of `Display`.

`backend` defaults to `http`. HTTP entries
accept `endpoint`, `api_key`, `api_key_command`, `protocol`, `temperature`,
`max_tokens` and
`max_retries`; they reject the Codex-only `reasoning_effort`. A `codex` entry
requires `model`, accepts an optional `reasoning_effort`, and rejects every
HTTP-only field, so a subscription selection cannot silently become API
traffic. It requires the separately installed Codex CLI and ChatGPT-managed
login; drep's endpoint-keyed auth store is not involved.

Validation is deliberately strict at load, because each of these can only fail
later and less legibly: a file declaring no providers, or none enabled, is
rejected; `max_concurrent = 0` is rejected, since a semaphore with no permits
would hang with no message. Disabled entries are inert - `${VAR}` expansion and
field validation both skip them, and so does credential resolution, which for an
`api_key_command` also means no subprocess - so parking a cloud provider does not
require its key to be set. `LlmConfig` hand-writes `Debug` to redact `api_key`,
and prints an `api_key_command` as its program name plus an argument count,
because an argv can carry the credential too.

### The site policy layer

`drep.toml` is per-repository and `drep init` gitignores it, so a control written there is per-developer and opt-in — which means off for the person who most needs it. A second layer sits above it, at `DREP_SITE_CONFIG` if set, else `/Library/Application Support/drep/site.toml` on macOS and `/etc/drep/site.toml` elsewhere. Deliberately not the `ProjectDirs` directory holding `auth.toml` and the cache: a policy file the policed developer can edit without privilege is not a policy file, and the same reasoning is why nothing in it is `${VAR}`-expanded.

A missing file is no policy and is not an error, because most machines have none; `site::load` returns an `Option` so a caller cannot confuse that with a policy permitting everything. A file that exists and cannot be read or parsed is **fatal, exit 2**. A policy that silently fails to load is worse than no policy, because the unconstrained run that follows reports as compliance. Unknown keys are rejected, which is also the whole of the "no providers, no credentials in this file" rule — an `[[llm]]` or an `api_key` there is an unknown key, so there is no separate rejection list to drift from the field list. `SiteConfigError` is its own enum rather than variants on `ConfigError`, so the error's type names which of the two files is at fault.

`max_concurrent_ceiling` lowers every enabled entry's `max_concurrent`: a checkout may lower its concurrency but not raise it past what the site allows. It applies to the effective value whether the repository wrote the field or inherited the default, since skipping the defaulted ones would let a repository raise itself by deleting a line. A ceiling of zero is rejected at site load, because the clamp runs after `config::validate` and would otherwise rebuild the no-permit hang that validation exists to prevent. A clamp is not an error; `doctor` reports it, on the provider it changes. `refuse_markers` is parsed and validated here — each entry must name one file — and is consumed by the marker refusal.

The clamp is applied by the caller, after `config::load` returns, which is what keeps `ConfigError` a statement about `drep.toml` alone: every one of its messages numbers `[[llm]]` entries in that file's order, and a bare `#2` that could mean either file is the ambiguity those messages exist to avoid. The policy is read before the repository config, so a broken policy cannot hide behind a broken `drep.toml`. Both paths are parameters threaded from the entry point, exactly as the auth store already is, so no test reads real machine state.

## Distribution

`dist-workspace.toml` drives cargo-dist. Four targets are built on two
repository-scoped homelab runners: Strix builds both Linux targets and owns
global plan, host and Homebrew publication work, while the arm64 Mac mini uses
the native macOS SDK to build both Apple targets. The generated
`.github/workflows/release.yml` is tag-only and creates the GitHub release and
Homebrew publication. crates.io remains a separate `cargo publish --locked`
operation because cargo-dist does not publish Rust crates. The arm64 Linux
runner mapping names Strix's x86_64 host explicitly; cargo-dist uses that host
fact to provision cargo-zigbuild and Zig instead of assuming native arm64.
`.github/build-setup.yml` installs pinned Zig 0.16.0 and cargo-zigbuild 0.23.0
for that matrix row before cargo-dist's generated dependency step. That avoids
the generated pip fallback, which Strix's PEP 668-managed Python rejects.
The same setup installs stable Rust plus the matrix-selected target on macOS
before cargo-dist is installed, so the Mac service never depends on an
interactive user's shell profile or a runner-global Cargo path.
Reqwest enables `native-tls-vendored` only for arm64 Linux, which compiles
OpenSSL for that target instead of requiring an arm64 OpenSSL sysroot on the
x86_64 host or adding the source build to native targets.

Cargo-dist's global jobs have two explicit Strix host prerequisites that its
generated workflow assumes are present on a GitHub-hosted image: `gh` must be
on the service PATH, and Homebrew publication uses Linuxbrew's supported
`/home/linuxbrew/.linuxbrew` prefix. The hardened service keeps home directories
hidden with `ProtectHome=tmpfs` and bind-mounts only that prefix read-write;
private user homes remain unavailable. The service PATH also includes its
dedicated Cargo bin directory because global jobs download a cached `dist`
there without adding that directory to `GITHUB_PATH`.

`.github/workflows/rust.yml` runs format, clippy, tests and the 1.88 MSRV check
in one Strix allocation, plus the test suite on the native Mac mini. Both
stable toolchains include Clippy: the test suite runs a real Rust fixture to
verify compiler-grounded semantic suppression. Its jobs
accept pushes and same-repository pull requests but skip forked pull requests
before a LAN runner is selected. `.github/workflows/mutants.yml` follows the
successful `rust` workflow for a push to `main` and checks out that exact SHA on
the same hardened Strix service labelled `drep-linux`; pull-request workflow
completions cannot trigger it. The mutation workflow keeps `target/` warm,
rejects any other persistent workspace state, and pins `cargo-mutants` 27.1.0;
`scripts/mutants-run.sh` remains the single definition of the mutation verdict.
