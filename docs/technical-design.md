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
| `cli/check/` | `input`, deterministic tools, `refusal`, `review_budget`, rendering and exit codes |
| `cli/mod.rs` | the command surface, the shared `--fail-on` parser, and `MachineFiles` |
| `cli/lint_docs/` | markdown command, `--staged`, `--fail-on` |
| `cli/doctor.rs` | what languages and tools are visible here, which providers, and what site policy does to them |
| `cli/doctor/llm.rs` | the `LLM analysis` block: the raw-file listing and the credential probe |
| `cli/doctor/site_section.rs` | the `Site policy` block and the per-provider clamp note |
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
| `config/site.rs` | the machine-level policy file, the concurrency ceiling, and the marker refusal |
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

A provider's credential is resolved in one pass, in `auth::resolve`, in this
order: an explicit `api_key`, then `api_key_command`, then the endpoint-keyed
store, then nothing — which `LlmClient::new` turns into an empty key so the SDK
sends no protocol authentication header. A configured header may provide the
endpoint's complete authentication scheme in that last case.
`auth::source_of` is the only statement of that order, and `doctor` calls it
rather than restating it. `api_key_command` is an argv run with no shell, whose
stdout trimmed at both ends is the credential — the trailing newline every
helper prints, and a leading space that a header value cannot carry anyway,
which `AuthStore::set` already trims off a pasted key. Setting it alongside
`api_key` is rejected at load, beside the unknown backend and the misspelled
protocol, because two answers to one question is not a precedence puzzle. A
malformed `${...}` reference reports only the shape of the defect, never the
argument that may contain a secret. A Codex entry rejects the command with the
other HTTP-only fields.

It resolves in that one pass, so each entry's command runs exactly once per
process: a short-lived credential re-minted per file fails per file, which the
chain's demotion logic reads as an endpoint problem. There is no disk cache and
no TTL behind it — drep is a short-lived process, so a credential on disk buys
nothing and adds a file worth stealing.

A failing command is fatal there, before the chain exists, which is what makes
it structurally unable to fail over. That is the same rule a 401 follows:
routing around a broken credential path is what hides it. The diagnostic names
the program and the exit status and nothing else, because a misconfigured
helper can print the token to either stream and an error message is the one
place it would escape. `KeyCommandError` carries no captured output, which is
what keeps that true of `{:?}` as well as of `Display`. Stdout is read with a
64 KiB ceiling while the direct child is polled for exit. That exit is decisive
even if a background grandchild inherited the pipe, so an unrelated descendant
cannot turn a successful helper into a timeout; the biased select and bounded
final drain retain bytes the helper wrote before exiting without waiting for
the descendant to close the pipe.

`backend` defaults to `http`. HTTP entries
accept `endpoint`, `api_key`, `api_key_command`, `headers`, `protocol`,
`temperature`, `max_tokens` and
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
require its key to be set. An unknown key in an entry is a load error rather than a silent drop, which is
what a `[llm.headers]` table written against a drep that could not send one used
to be: accepted, discarded, and indistinguishable from working. `LlmConfig`
hand-writes `Debug` to redact `api_key`, prints an `api_key_command` as its
program name plus an argument count, and prints `headers` as its names alone,
because an argv and a header value can each carry the credential too.

`[llm.headers]` and drep's own `User-Agent: drep/<version>` are merged once, by
`config::effective_headers` at `LlmClient::new`, and the result is stored as
`LlmClient::headers`. The default is only created when the configured table
names no user agent, matched case-insensitively, so an entry that sets one gets
one header rather than two. `complete_json` applies that resolved map and
settles no precedence of its own, which is what keeps the request, `LlmClient`'s
`Debug` and `drep doctor` answering the same question the same way: with the
default applied inside the request instead, a config naming one header printed
one and sent two, and a config naming none printed an empty set and still sent a
user agent - and the operator debugging a gateway 403 is asking `doctor`
exactly that. `doctor` marks a name the entry did not write as `(default)`,
comparing the configured set against the effective one rather than naming
`User-Agent` itself, so a second default cannot be added without that listing
reporting it. The resolved map still goes on after the SDK's protocol defaults,
and `AgentOptionsBuilder::header` replaces case-insensitively, so a configured
`Authorization` is the one sent. That replacement never has to choose between
two *configured* spellings, because `validate` refuses a table holding both:
the two are equal keys to HTTP and distinct keys to a `BTreeMap`, so the
surviving one would be chosen by byte order while `doctor` and both `Debug`
impls went on listing the other.

The cache key carries a conservative request identity: protocol, `max_tokens`,
and the effective header set, in addition to endpoint, model, prompt, and
temperature. Header names are canonicalised case-insensitively and values
remain exact hash inputs. drep cannot tell whether an arbitrary header is only
a credential or selects a tenant, route, or feature variant, so a rotation
cold-starts the cache rather than reusing an answer from a request that may have
reached different backing behavior.

Every HTTP endpoint is an exact origin rather than the start of a redirect
chain. `open-agent-sdk` 0.11.2 disables redirects for both completion request
paths; `http::client` independently disables them for drep's model-listing and
quirks fetchers. This is deliberately stricter than stripping selected headers
only on a cross-origin hop: an Anthropic `x-api-key` is not one of reqwest's
built-in sensitive names, and a same-origin redirect would replay even a
standard `Authorization` header. A `30x` therefore reaches the caller's normal
status classification and no second request is made.

### The site policy layer

`drep.toml` is per-repository and `drep init` gitignores it, so a control written
there is per-developer and opt-in — which means off for the person who most
needs it. A second layer sits above it, at
`/Library/Application Support/drep/site.toml` on macOS and
`/etc/drep/site.toml` elsewhere. Deliberately not the `ProjectDirs` directory
holding `auth.toml` and the cache: a policy file the policed developer can edit
without privilege is not a policy file, and the same reasoning is why nothing
in it is `${VAR}`-expanded.

`DREP_SITE_CONFIG` names the file only when none is installed at that path,
which is what an installation keeping it elsewhere needs and what every test
needs. It cannot displace an installed one: an override that could would leave
the layer one `export` away from off, and `refuse_markers` — rejected in
`drep.toml` on the grounds that a refusal a developer can delete is not one —
would be deletable by exporting a path to an empty file. Presence is
`symlink_metadata`, matching the marker probe. Only `NotFound` means the
machine path is absent; every other metadata result keeps it authoritative. A
dangling symlink therefore does not hand the decision back to the environment,
and its later read failure is fatal rather than collapsed into no policy. The
precedence is visible rather than silent: `doctor` names the file in effect.

A missing file is no policy and is not an error, because most machines have
none; `site::load` returns an `Option` so a caller cannot confuse that with a
policy permitting everything. A file that exists and cannot be read or parsed
is **fatal, exit 2**. A policy that silently fails to load is worse than no
policy, because the unconstrained run that follows reports as compliance.
Unknown keys are rejected, which is also the whole of the "no providers, no
credentials in this file" rule — an `[[llm]]` or an `api_key` there is an
unknown key, so there is no separate rejection list to drift from the field
list. `SiteConfigError` is its own enum rather than variants on `ConfigError`,
so the error's type names which of the two files is at fault.

`max_concurrent_ceiling` lowers every enabled entry's `max_concurrent`: a
checkout may lower its concurrency but not raise it past what the site allows.
It applies to the effective value whether the repository wrote the field or
inherited the default, since skipping the defaulted ones would let a repository
raise itself by deleting a line. A ceiling of zero is rejected at site load,
because the clamp runs after `config::validate` and would otherwise rebuild the
no-permit hang that validation exists to prevent. A clamp is not an error;
`doctor` reports it, on the provider it changes. `refuse_markers` is parsed and
validated here — each entry must name one file — and is consumed by the marker
refusal.

The clamp is applied by the caller, after `config::load` returns, which is what
keeps `ConfigError` a statement about `drep.toml` alone: every one of its
messages numbers `[[llm]]` entries in that file's order, and a bare `#2` that
could mean either file is the ambiguity those messages exist to avoid.
`check::configured` is that caller — one named function rather than three steps
in the orchestrator, so a test can observe the ceiling reaching a config a real
run would use. The policy is read before the repository config, so a broken
policy cannot hide behind a broken `drep.toml`. Both paths are parameters
threaded from the entry point as one `cli::MachineFiles`, exactly as the auth
store already was, so no test reads real machine state and no caller can
transpose two same-typed positionals — a swap that silently leaves the fleet
policy unapplied while the run reports as compliance.

Every field declared by `SiteConfig` is site-only. `config::load` reads
`refuse_markers` and `max_concurrent_ceiling` off the raw tree and rejects
either in `drep.toml` with `ConfigError::SiteOnlyField`. Rejected rather than
ignored is the invariant: `Config` has no such fields and no
`deny_unknown_fields`, so serde would otherwise drop a key silently and a
developer would read their own config and believe a policy was active. A
repository can already lower an individual provider's `max_concurrent`, but a
silently ignored fleet ceiling is still a false claim of enforcement. An
exhaustive destructure beside `SITE_ONLY_FIELDS` makes a new site field fail to
compile until its repository-file treatment is decided.

### The marker refusal

`SiteConfig::refusal_among` returns the first configured marker present at any
repository containing bytes this run would send, and `cli/check/refusal.rs` is
the caller in the gate. Its ordering is the feature: the probe runs before the
credential store is opened, before any `api_key_command` subprocess, and before
`ProviderChain::new`, which for a `codex` entry spawns the CLI to read login
state. It also precedes every cache read, and cache construction itself is
side-effect-free, so `--cache-only` and `--push-gate` cannot serve a model's
verdict, create a cache directory or evict an entry for a repository whose
source was never allowed to reach one. `Source` has two arms and no arm holding
both a refusal and an analyzer, which makes "refused implies no chain"
structural rather than a convention two call sites keep.

An empty `refuse_markers` short-circuits before git is spawned, so a machine
that installed no marker policy gains neither the latency nor a new failure
mode. A machine that did installs a fail-closed one: a policy naming markers
outside a repository is `SiteConfigError::MarkerRootUnresolved`, because
"cannot be evaluated" must not become "evaluates to allowed". The marker probe
likewise treats only `NotFound` as absence; another metadata error is
`MarkerUnreadable`, not permission.

The repositories probed are those containing source the run would send, not the
process root alone: `files::expand_named` applies no confinement to `root`, so
`drep check <absolute path>` and a marked checkout nested inside an unmarked one
can otherwise review one repository's source while consulting another's policy.
Input resolution therefore runs before the probe — it reads local files and
contacts nothing, so the ordering above is untouched. Paths mode canonicalizes
each explicit target, reads from that same canonical path and records its target
directory; this makes the target repository decide when a source symlink
crosses the boundary in either direction. Diff modes record hunk parents
because a deleted path cannot be canonicalized. Each directory is resolved
through git and the marker checked once per distinct root; deciding from lexical
paths alone would reimplement git's discovery rules and reopen the symlink
bypass. Any marked repository in the set refuses the whole run, which is what
keeps `Source` two-armed.

The repository root comes from `diff::repository_root`, so the query goes
through the one place drep spawns git and inherits its
`GIT_DIR`/`GIT_WORK_TREE` scrubbing - a marker checked against the tree a hook's
environment named instead of the tree being checked is a policy bypass. The
marker has to be committed to be present in every checkout: git materialises
tracked content, so an untracked one is absent from a fresh clone and from a
linked worktree. Presence is `symlink_metadata`, not `metadata` and not
`is_file`: a directory, or a symlink whose target is gone, is still a name
someone deliberately placed, and either narrower reading is a way to disable
the policy while appearing to invoke it. Nothing opens the file.

`doctor` uses the parent directories of the source files it discovers, with the
process root only as the fallback diagnostic when there is no semantic payload.
It carries permitted, refused and unevaluable as three states. Only the first
may open the auth store, run `api_key_command` or probe Codex; the other two
report why setup was not attempted, matching the gate instead of spending a
credential to describe a run that cannot occur.

The refusal arrives as `FailureReason::SitePolicyRefused`, one entry per file
drep was asked about, which is what makes `gate`, the text failure block, the
JSON `unanalyzed` array and the clean-cycle reset guard all treat it correctly
with no second mechanism. `semantic::refused` claims no review round and leaves
`should_review_live` false, so a refused run is structurally unable to reach the
exit-3 push handshake. Deterministic tools still run and still gate;
`lint-docs` reads no config and is untouched.

## Distribution

`dist-workspace.toml` drives cargo-dist. Four targets are built on two
repository-scoped homelab runners: homelab-1 builds both Linux targets and owns
global plan, host and Homebrew publication work, while the arm64 Mac mini uses
the native macOS SDK to build both Apple targets. The generated
`.github/workflows/release.yml` is tag-only and creates the GitHub release and
Homebrew publication. crates.io remains a separate `cargo publish --locked`
operation because cargo-dist does not publish Rust crates. The arm64 Linux
runner mapping names homelab-1's x86_64 host explicitly; cargo-dist uses that host
fact to provision cargo-zigbuild and Zig instead of assuming native arm64.
`.github/build-setup.yml` installs pinned Zig 0.16.0 and cargo-zigbuild 0.23.3
for that matrix row before cargo-dist's generated dependency step. That avoids
the generated pip fallback, which homelab-1's PEP 668-managed Python rejects.
The same setup installs stable Rust plus the matrix-selected target on macOS
before cargo-dist is installed, so the Mac service never depends on an
interactive user's shell profile or a runner-global Cargo path.
Reqwest enables `native-tls-vendored` only for arm64 Linux, which compiles
OpenSSL for that target instead of requiring an arm64 OpenSSL sysroot on the
x86_64 host or adding the source build to native targets.

Cargo-dist's global jobs have two explicit homelab-1 host prerequisites that its
generated workflow assumes are present on a GitHub-hosted image: `gh` must be
on the service PATH, and Homebrew publication uses Linuxbrew's supported
`/home/linuxbrew/.linuxbrew` prefix. The hardened service keeps home directories
hidden with `ProtectHome=tmpfs` and bind-mounts only that prefix read-write;
private user homes remain unavailable. The service PATH also includes its
dedicated Cargo bin directory because global jobs download a cached `dist`
there without adding that directory to `GITHUB_PATH`.

`.github/workflows/rust.yml` runs format, clippy, tests and the 1.88 MSRV check
in one homelab-1 allocation, plus the test suite on the native Mac mini. Both
stable toolchains include Clippy: the test suite runs a real Rust fixture to
verify compiler-grounded semantic suppression. Its jobs
accept pushes and same-repository pull requests but skip forked pull requests
before a LAN runner is selected. Both validation jobs disable rust-cache's
Cargo binary caching. The action's default save cleanup removes every binary
that existed when the job began, but these self-hosted runners keep pinned
publisher tools in that same Cargo bin directory. Registry, Git and target
caching remain enabled without allowing validation to delete host-owned tools.
After the Linux and macOS jobs accept a trusted push to `main`, `rust.yml`
mutates only production code in the complete pushed diff on the dedicated
Legion runner. It fetches full history so `github.event.before` is an exact
base rather than assuming one pushed commit. `.github/workflows/mutants.yml`
owns the exhaustive sweep: it runs weekly or by explicit dispatch, refuses a
manual ref outside the default branch, and never triggers for a push or pull
request. Both mutation lanes keep `target/` warm, reject any other persistent
workspace state, and pin `cargo-mutants` 27.1.0. `scripts/mutants-run.sh`
remains the single definition of the verdict.

Developer offload through `scripts/mutants-remote.sh` uses the SSH account's
`~/.cache/drep-mutants/<repo>` tree instead of the protected runner checkout.
It defaults to Legion's reserved Ethernet address and shares the host lock with
hosted mutation, so the two entrypoints cannot run concurrently. Homelab-2 is
no longer a Drep mutation owner.
