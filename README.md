<div align="center">

# drep

<img src="docs/images/drep.png" alt="drep logo" width="200" />

**A local commit gate.** It runs the linters your repository already
configures, and sends the code you changed to an LLM for review.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/slb350/drep/releases/latest/download/drep-ai-installer.sh | sh
```

</div>

## What it does

On `git commit` and `git push`, drep checks your changes twice.

| Layer | Source | Blocks? |
|---|---|---|
| Deterministic | ruff, eslint, tsc, gofmt, go vet, clippy | Yes |
| Semantic | an LLM you point it at | No, unless you ask |

Your linters are precise, so their findings block. A model's opinion about
naming is not precise at any severity, so it informs instead. Splitting by
source rather than by severity is what makes the gate calibratable; opt the
LLM into blocking with `--fail-on error` when you want it.

Semantic review is intentionally high-signal rather than exhaustive. The model
is asked for concrete, reachable defects worth fixing before merge, and told
to omit speculative hardening, implausible extreme edge cases, nits and cleanup
opportunities. A clean response is preferable to manufacturing marginal work.

drep is a single binary. It talks to no source-control platform, runs no
server, and needs no drep account. The deterministic half needs no model and
no API key at all. Semantic review can use an HTTP API or a separately
installed Codex CLI authenticated through a ChatGPT subscription.

## Install

```sh
# Shell installer (macOS and Linux, x86_64 and arm64)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/slb350/drep/releases/latest/download/drep-ai-installer.sh | sh

# Homebrew
brew install slb350/tap/drep

# crates.io
cargo install drep-ai

# Current main branch
cargo install --git https://github.com/slb350/drep drep-ai
```

The crate is `drep-ai` because `drep` was taken on crates.io. The binary is
`drep`.

## Set up a repository

```sh
cd your-repo
drep init                              # interactive: pick a provider, paste a key,
                                       # choose from the models it actually serves
drep init --provider openrouter        # HTTP API
drep init --provider codex             # ChatGPT/Codex subscription
# Other presets: local, zai, minimax, kimi, openai, custom
export OPENROUTER_API_KEY='...'
```

`drep init` writes two things: a `drep.toml` naming your model, and a git hook.
The default is a pre-push hook; `--hooks pre-commit` or `--hooks both` if you
want the gate earlier, `--hooks none` for the config alone.

The interactive path fills in `temperature` and `max_tokens` for the model you
picked rather than for its provider, using a weekly-refreshed copy of
[models.dev](https://models.dev). It only ever removes a parameter or lowers a
required ceiling, never the reverse, so a model that refuses `temperature` gets
no `temperature` line and `k3` gets its own 131,072 instead of a number chosen
for the endpoint. Offline, or for a model too new to be listed, the provider's
defaults are written and setup carries on. `DREP_QUIRKS_PATH` points drep at a
different cache.

It writes native git hooks rather than a pre-commit entry, and it handles
`core.hooksPath`: if you have a global hooks directory, a repository-local hook
would otherwise never fire, silently.

The pre-push hook protects Git's already-open remote connection from a long
cold review. When every review is cached, the push continues immediately. On a
cache miss, drep completes and caches the review but deliberately stops that
push with exit 3; run `git push` again and the cache-only retry reconnects and
finishes quickly. Findings and analysis failures keep their normal exit codes,
so only a successful cache warm asks for the retry. Fresh semantic remediation
is bounded to three finding-producing rounds by default; cached reviews remain
available after that limit.

To adopt drep through [pre-commit](https://pre-commit.com) instead:

```yaml
repos:
  - repo: https://github.com/slb350/drep
    rev: v2.6.1
    hooks:
      - id: drep-check-push   # pre-push: what the push touches
      # - id: drep-check      # pre-commit: staged files
      # - id: drep-lint-docs  # markdown, rule-based
```

The pre-commit pre-push hook reads the base and pushed tip that pre-commit
exports and disables filename arguments. This keeps review in diff-hunk mode;
passing the filenames would make a one-line follow-up fix re-review each whole
file.

## Commands

```sh
drep check                      # this directory
drep check src/ main.go         # named files or directories
drep check --staged             # what is staged, for a pre-commit hook
drep check --diff origin/main   # what changed since a ref, for pre-push
drep check --cache-only         # cached LLM reviews only; miss exits 3
drep check --push-gate          # warm cold reviews, then ask for a fresh push
drep check --max-review-rounds 5 # authorize a larger remediation cycle
drep check --unlimited-reviews  # explicitly remove the limit for this run
drep check --fail-on error      # also block on LLM findings
drep check --format json        # machine-readable
drep acknowledge <fingerprint>  # hide a reviewed false positive until code changes

drep lint-docs                  # markdown in this tree
drep lint-docs --staged --fail-on error
drep doctor                     # what will actually run here
```

Exit codes (`3` is specific to `check`):

| Code | Meaning |
|---|---|
| 0 | Everything that should have run, ran, and found nothing blocking |
| 1 | Blocking findings |
| 2 | Something that should have run did not |
| 3 | Cache-only miss, or a successful push-gate warm requiring a fresh push |

In JSON, `retry_push: true` identifies the successful warm-and-reconnect case;
a plain `--cache-only` miss leaves it false. The `review` object reports a
counted round, reset, or explicitly unlimited review. Reaching the fresh-review
limit is an unanalyzed result and exits 2 rather than silently passing.

Exit 2 is the one that matters. An unreachable endpoint, a file too large for
the model, a configured tool that is not installed, a repository whose site
policy refuses semantic review: none of those are a pass,
and a gate that reports them as one is worse than no gate.

## What will run here

```console
$ drep doctor
drep in /Users/you/your-repo
============================================================

Languages found:
  Go: 12 file(s)         Python: 48 file(s)      TypeScript: 31 file(s)

Deterministic checks (these gate):
  ruff: ready
  gofmt: ready
  go vet: ready
  eslint: not configured (add one of: eslint.config.js, ...)
  tsc: configured but NOT INSTALLED - these checks will not run

LLM analysis (required):
  1. deepseek/deepseek-v4-pro-0813 at https://openrouter.ai/api/v1
```

Three rules decide what runs:

- Repository-local before PATH, so a project is checked by the version its CI
  runs.
- From the nearest configured ancestor of each file. A monorepo member's
  eslint or TypeScript config applies to that member, while a hoisted binary at
  the repository root remains usable.
- A configured tool that is missing makes the run exit 2. A check that did not
  run is never reported as a pass.

## Languages

| Language | Extensions | Tools |
|---|---|---|
| Python | `.py` | ruff |
| JavaScript | `.js` `.jsx` `.mjs` `.cjs` | eslint |
| TypeScript | `.ts` `.tsx` `.mts` `.cts` | eslint, tsc |
| Go | `.go` | gofmt, go vet |
| Rust | `.rs` | clippy |

The LLM half reads any of them. It parses nothing, so it needs no grammar per
language; it is told which language it is reading and which conventions that
language's ecosystem expects.

LLM findings print an acknowledgement command. Running it records the finding
fingerprint in `.drep/acknowledgements.toml`; commit that file when the team's
adjudication should be shared. The fingerprint includes the file, finding
category and surrounding source, so an edit near the finding makes it eligible
for review again.

### Autonomous remediation

drep enforces at most three fresh, finding-producing semantic remediation
rounds per branch and worktree by default. `--staged`, `--diff`,
`--pre-commit-push`, and a bare `--push-gate` participate; named-path checks do
not. A round is retained only when an uncached provider response still has an
actionable finding after compiler-grounded suppression and acknowledgements.
Clean responses and pure analysis failures refund their reservation, while a
clean complete diff or push-gate check resets the completed cycle.

At the limit, deterministic checks and cached LLM verdicts still run, but a
cold semantic cache miss exits 2 without contacting a provider. Raise the
top-level `max_review_rounds`, pass `--max-review-rounds N`, or explicitly pass
`--unlimited-reviews` when a longer cycle is warranted. State is private to the
worktree's Git metadata and pending reservations expire after a crashed run.

## Markdown

`drep lint-docs` is rule-based only: no LLM, no network, no config file. Ten
checks, and their severity answers one question, which is whether the finding
changes how the document renders.

| Severity | Checks |
|---|---|
| error | `unclosed_code_fence` |
| warning | `empty_heading`, `missing_space_after_heading`, `link_syntax_invalid` |
| info | `bare_url`, `long_line`, `tab_character`, `trailing_whitespace` |
| info | `trailing_blank_lines`, `multiple_blank_lines` |

An unclosed fence turns every line below it into code, so it alone blocks by
default. Whitespace renders identically, so it does not. `--strict` is the
shorthand for `--fail-on info`, which blocks on everything; over a real
repository that is dominated by line length, and a hook that blocks a commit
over a long line is a hook that gets deleted.

## Configuration

`drep.toml`, written by `drep init`:

```toml
max_review_rounds = 3

[[llm]]
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-v4-pro-0813"
api_key = "${OPENROUTER_API_KEY}"
timeout_secs = 1800
```

`max_review_rounds` must be at least 1. It defaults to 3 when omitted, so older
configurations receive the bounded behavior without regeneration.

HTTP is the default backend, so existing configurations remain valid without a
`backend` field. OpenAI API usage is the `openai` preset and uses per-token API
billing:

```sh
drep init --provider openai
export OPENAI_API_KEY='...'
```

ChatGPT/Codex subscription usage is a separate backend. Install the official
[Codex CLI](https://learn.chatgpt.com/docs/codex/cli), follow the
[Codex authentication](https://learn.chatgpt.com/docs/auth) guide to run
`codex login`, then select the `codex` preset:

```sh
codex login
drep init --provider codex
```

```toml
[[llm]]
backend = "codex"
model = "gpt-5.6-sol"
reasoning_effort = "high"
timeout_secs = 1800
max_concurrent = 1
```

This mode consumes the ChatGPT/Codex plan allowance, not OpenAI API credits.
drep never reads or stores the subscription tokens: Codex owns login and token
refresh. Every review is an ephemeral, non-interactive, read-only Codex run in
an empty directory with tools, apps, MCP, hooks, memories, user configuration,
and project instructions disabled. `drep doctor` verifies the installed CLI
and ChatGPT-managed authentication without printing account details.

`drep init` does not write your key into this file. Keys go to a per-machine
store (`~/.config/drep/auth.toml`, mode 0600, keyed by endpoint), so `drep.toml`
carries only the provider choice:

```sh
drep auth list                                   # endpoints held; never the keys
drep auth login --provider kimi                  # paste a key, no echo
drep auth logout --endpoint https://api.kimi.com/coding/v1
```

`api_key = "${VAR}"` still works and takes precedence over a stored key, which
is what CI wants — there is nobody to paste anything there. `DREP_AUTH_PATH`
points drep at a different store.

### Fleet-managed policy

`drep.toml` is per-repository, and `drep init` gitignores it, so anything you put there is a per-developer choice. A machine-level policy file sits above it:

```toml
# /Library/Application Support/drep/site.toml   (macOS)
# /etc/drep/site.toml                           (everywhere else)
max_concurrent_ceiling = 4
refuse_markers = [".drep-no-llm"]
```

A repository can lower its own `max_concurrent`, but not raise it past the ceiling. `DREP_SITE_CONFIG` points drep at a different file. The location is deliberately a system path rather than your config directory: a policy file the developer can edit without privilege is not a policy file, which is also why nothing in it is `${VAR}`-expanded.

No file means no policy, which is the normal state. A file that exists and drep cannot read or parse exits 2 rather than running without it — a policy that silently fails to load is worse than none, because the unconstrained run reports as compliance. Unknown keys are rejected, so a typo is loud, and there is no `[[llm]]`, no endpoint and no credential in this file. `drep doctor` says whether a policy is in effect, which file it came from, and which providers it lowered.

### Repositories whose source must never reach a model

`refuse_markers` names files whose presence at a repository's root means exactly that. Create `.drep-no-llm` at the root of such a checkout and `drep check` will not send it to a provider — not on a fresh review, and not as a lookup of a review it already has cached.

Presence is the whole signal. drep never opens the marker, so its contents cannot say `allow`, and a directory or a broken symlink bearing the name counts too: either would otherwise be a way to switch the policy off while appearing to invoke it. Only the repository root is consulted, resolved through git, so a check run from a subdirectory is refused as well and a vendored copy of the filename deeper in the tree changes nothing. Each marker must name one file; a path with a separator in it is rejected when the policy loads.

A refused semantic review is an unanalyzed result and exits 2. It is not a pass and not a quiet skip: `--format json` reports it in `unanalyzed` with `kind: "site_policy_refused"` beside the marker and policy paths, and the text output names the marker that was found. Deterministic tools still run and still gate — they are local, they contact nothing, and they are the half of drep that works without a model. `drep lint-docs` is rule-based and unaffected.

The field is readable only from the site file. It is rejected in `drep.toml`, because `drep init` gitignores that file: a copy of the control there would be per-developer and deletable by the developer it constrains, and a control that is silently ignored is worse than one that was never written.

### Credentials that expire in minutes

Some gateways hand out short-lived tokens: a stored key is stale before the second commit, and a `${VAR}` is stale before the shell that exported it is closed. `api_key_command` is an argv drep runs to mint one, and its whole trimmed stdout is the credential:

```toml
[[llm]]
endpoint = "https://gateway.example/v1"
model = "m"
api_key_command = ["gcloud", "auth", "print-access-token"]
```

Anything that prints a token works — `gcloud auth print-access-token`, `az account get-access-token`, `op read`, `vault read`. It is an argv array, not a shell line: there is no `sh -c`, so no quoting, globbing or `$(...)` to get wrong. `${VAR}` inside an element is expanded like anywhere else in the file.

The order is now: an explicit `api_key`, then `api_key_command`, then the stored key, then nothing. Setting both `api_key` and `api_key_command` on one entry is a config error rather than a precedence puzzle, and `backend = "codex"` rejects `api_key_command` along with the other HTTP-only fields.

The output is taken whole, with only trailing whitespace removed. drep does not look for a line or a `token=` prefix in it, so a helper that prints anything other than the credential needs a wrapper that prints only the credential.

A command that fails is fatal, and exits 2. It does not fall through to the next provider, for the reason a 401 does not: routing around a broken credential path is what hides it. The failure names the program and its exit status and nothing else — a misconfigured helper can print the token to stdout or stderr, and an error message is the one place it would escape into a terminal, a CI log or a bug report.

It runs once per process, and the result is never written to disk. `drep doctor` really runs it, because doctor's job is to report what will actually happen here — so a helper behind a biometric or approval prompt will prompt on every `drep doctor`. That line says whether the command succeeded, never what it printed.

By default `drep init` also adds `drep.toml` to `.gitignore`; pass
`--no-gitignore` to commit it instead and share the provider choice with the
repository.

`[[llm]]` is an array of tables, and the order is a failover chain. Each entry
is tried in turn: a timeout, a refused connection, a 429, a 5xx or an empty
answer falls through to the next one. A 401 or 403 does not, because that is a
broken key and falling back would hide it. Set `enabled = false` on an entry to
park it without deleting it.

```toml
# Local model first, cloud when it is not running.
[[llm]]
endpoint = "http://localhost:1234/v1"
model = "qwen3-30b-a3b"

[[llm]]
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-v4-pro-0813"
api_key = "${OPENROUTER_API_KEY}"
```

### Subscription coding plans

`drep init` has presets for three of them, so a plan you already pay for can run
the gate instead of per-token API billing:

```sh
drep init --provider zai       # GLM 5.3, OpenAI-compatible
drep init --provider minimax   # MiniMax M3, Anthropic protocol
drep init --provider kimi      # Kimi k3, Anthropic protocol
```

Two of those endpoints expose the Anthropic messages API rather than chat
completions, which is what `protocol` selects:

```toml
[[llm]]
endpoint = "https://api.minimax.io/anthropic/v1"
model = "MiniMax-M3"
api_key = "${MINIMAX_API_KEY}"
protocol = "anthropic"
```

`protocol` defaults to `openai`, so an existing file needs no change. `doctor`
tags a non-default protocol in its listing.

Check the plan's own terms before pointing a commit gate at it. They are not
uniform, and some restrict a subscription to a named list of client tools or to
interactive use.

### Other keys

`temperature` (unset means the parameter is not sent at all, which is what some
models require — `k3` and `gpt-5.6-sol` reject any value), `max_tokens` (unset
by default, so a reasoning model is never truncated mid-thought; a few endpoints
refuse a request without it), `max_retries`, `max_concurrent`.

Both are properties of the model rather than the endpoint, which is why the
wizard resolves them per model. Editing either by hand always wins: drep reads
the file as written and never revisits it.

## Development

```sh
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features     # levels come from [lints] in Cargo.toml
cargo mutants                                 # a green suite is not a discriminating one
```

`docs/technical-design.md` is the architecture. `CLAUDE.md` carries the
invariants, each with the defect that produced it.

## License

MIT. See [LICENSE](LICENSE).
