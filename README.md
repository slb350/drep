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
so only a successful cache warm asks for the retry.

To adopt drep through [pre-commit](https://pre-commit.com) instead:

```yaml
repos:
  - repo: https://github.com/slb350/drep
    rev: v2.5.0
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
a plain `--cache-only` miss leaves it false.

Exit 2 is the one that matters. An unreachable endpoint, a file too large for
the model, a configured tool that is not installed: none of those are a pass,
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

An agent that fixes advisory LLM findings should default to at most three
drep-driven remediation rounds for one change set. After that, it should still
fix deterministic failures and analysis failures, but hand any new advisory
LLM findings to a person (or acknowledge a confirmed false positive) instead
of continuing automatically. This is an orchestrator policy, not a drep
shutoff: drep keeps reviewing every pushed change, because a fourth review can
contain a real regression and a hidden counter must never wave it through.

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
[[llm]]
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-v4-pro-0813"
api_key = "${OPENROUTER_API_KEY}"
timeout_secs = 1800
```

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
