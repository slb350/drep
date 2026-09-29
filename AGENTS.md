# AGENTS.md

Instructions for coding agents working in this repository. This file is tracked and authoritative. A local `CLAUDE.md` stays gitignored; it imports this file with `@AGENTS.md` and may add private notes.

## What drep is

drep is a Rust binary: a local commit gate that runs the linters and
formatters a repository already configures, and sends the code that changed to
an LLM for review. It runs on pre-commit and pre-push. It is a local process
with no server or background service. Persistent state is limited to local
configuration, credentials, model metadata, acknowledgements and response
cache entries, plus worktree-local semantic-review round leases under Git
metadata.

- Architecture: [docs/technical-design.md](docs/technical-design.md)
- Everything below is invariants, commands and contracts. The narrative belongs
  in [CHANGELOG.md](CHANGELOG.md).

## Commands

```sh
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features   # levels come from [lints], not from flags
cargo fmt --all
cargo build --release                       # the git hooks run ./target/release/drep

./scripts/mutants-staged.sh                 # the staged diff, what the hook runs
./scripts/mutants-remote.sh                 # an explicit full sweep on homelab-ai-1
./scripts/mutants-run.sh                    # an explicit local full sweep

./target/release/drep doctor                # what will run in this repository; an
                                            # `anthropic` provider is tagged in the listing
./target/release/drep check --staged
./target/release/drep lint-docs --fail-on error
```

Distribution, when `dist-workspace.toml` changes:

```sh
dist init                                   # regenerates .github/workflows/release.yml
dist plan
dist build --artifacts=local --target=aarch64-apple-darwin
```

### The local gate

`pre-commit` drives the hooks and lives on PATH: `uv tool install pre-commit`.
Installing the hooks needs a dance, because the global `core.hooksPath`
(`~/.git-hooks`) chains to the repository-local hook and pre-commit refuses to
install while it is set:

```sh
git config --local core.hooksPath "" \
  && pre-commit install \
  && pre-commit install --hook-type pre-push
git config --local --unset core.hooksPath
```

Leave the global `core.hooksPath` alone: unsetting it kills the global
post-commit backup of gitignored files. With it set, git looks **only** in
`~/.git-hooks`, so a repository-local hook runs only if a chainer of the same
name exists there. The `pre-push` chainer must `exec`, which is how the local
hook inherits stdin and therefore the refs being pushed.

The pre-push hook sends each changed file to the review backends that the local, gitignored `drep.toml` selects, and needs only the credential those entries name: none for a credentialless endpoint or the keyless `codex` preset, otherwise the `${VAR}` an entry's `api_key` references or a key stored with `drep auth`. When that key is kept in the gitignored `.env`, export it for the push:

```sh
set -a && . ./.env && set +a && git push origin HEAD
```

The gate can take minutes per file and consumes the selected backend's quota, which is
why it is pre-push and not pre-commit. A cold push now completes and caches the
review, deliberately exits 3, and tells you to run `git push` again; that retry
opens a fresh remote connection and uses cache-only verdicts. This prevents an
SSH connection opened before the hook from sitting idle for the whole review.
**Do not modify any file while a push is running**: pre-commit hashes the tree
before and after, and fails the hook with "files were modified by this hook"
for a change made by something else.

`drep.toml` is per-checkout provider selection and is gitignored. Never commit
it: one checkout may use Kimi while another uses Codex, OpenRouter or a local
server, and committing one machine's gate configuration changes every other
developer's provider, quota and credential requirement.

## Invariants

Each of these is a rule with a defect behind it. Changing one means the defect
comes back.

- **Mutation scope follows the trigger.** The local hook tests the staged diff;
  CI tests the complete diff of a trusted `main` push, or of a pull request from a branch of this repository (base to head, on the head tree), after Linux and macOS validation pass; fork pull requests never reach the mutation runner. Full sweeps run separately. `scripts/mutants-run.sh`
  owns the pass/fail rule for all three. A surviving mutant is a test that cannot tell correct from
  incorrect behaviour - fix the test, never exclude the mutant. Mutation
  tests for permission hardening must establish the insecure mode explicitly,
  not rely on the process umask: the former Strix runner used `UMask=0077`, which once
  made a deleted `restrict(..., 0o700)` call look correct in the clean CI
  environment while the same mutant was caught under a developer shell.
- **Explicit offloaded sweeps run on homelab-ai-1 (`192.168.68.88`), not here.** For local hooks,
  `scripts/mutants-remote.sh` syncs the tree, runs `mutants-run.sh` there and
  propagates its exit code, mirroring `target/mutants/` back. Its SSH-owned
  worktree is `~/.cache/drep-mutants/<repo>`, separate from protected runner
  checkouts under `/srv/ci`; the root-owned offload helper executes both rsync
  and commands as `ci-drep-mutants` inside the hosted runner's bounded
  `ai-ci.slice` sandbox. The host lock serializes concurrent explicit
  sweeps. Tests that invoke `mutants-run.sh` as a child must remove inherited
  host-lock, wait and result-token variables: inheriting the outer process's
  lock makes the child wait on its own parent and prevents the unmutated
  baseline from completing. An unreachable host falls back to a local hook run
  *with a warning*, never a silent skip. `DREP_MUTANTS_REMOTE=0` forces local.
  The hook sync is
  `--delete --force --delete-excluded` with a
  `P /target` filter: an excluded name *inside* a directory protects it from
  deletion, so without `--delete-excluded` a deleted directory survives on the
  remote and the sweep tests a tree the commit does not have; without the
  protect rule, `--delete-excluded` takes `target/` and every run is a cold
  build. `scripts/mutants-common.sh` holds the one definition of
  `target/mutants` and the host-lock wait default and validation shared by the
  remote and direct runners; a file the sync would skip is named by the caller in
  `MUTANTS_EXTRA_FILES` rather than recovered by the transport script parsing
  cargo-mutants' flags. The remote command appends its `%q`-quoted arguments
  only when at least one exists: formatting an empty `"$@"` produces the
  literal argument `''`, which makes the no-argument full sweep fail usage
  before its baseline. Raising `MUTANTS_JOBS` is a trap: every job copies the
  tree including `target/`, so the same scope measured 38s at `-j 4`, 54s at `-j 8`
  and 72s at `-j 16` on a 32-thread box. It is I/O-bound, not CPU-bound.
  Those per-job copies go to `$TMPDIR`, which `mutants-run.sh` sets to a
  sibling of the checkout (`<checkout>.mutants-tmp`, override with
  `DREP_MUTANTS_TMPDIR`) and sweeps of stale `cargo-mutants-*.tmp` before and
  after each run. cargo-mutants deletes its copies only on a clean exit, so a
  cancelled or timed-out job strands them. The former Strix host mounted
  `/tmp` as tmpfs, where five killed runs once pinned 31 GiB of RAM; the
  dedicated homelab-ai-1 path is disk-backed. A sibling rather than a child of the checkout
  because the copy excludes only `mutants.out`, so a copy under `target/`
  would be copied into every later one. Cleanup uses a depth-first,
  prefix-scoped `find -delete` for both `cargo-mutants-*.tmp` and interrupted
  child tests' `drep-diff-test-*` repositories, and never invokes `rm` or
  `rmdir`; broad recursive removal commands have damaged this server before.
  Do not move the scratch root back or weaken the cleanup boundary.
- **Every GitHub Actions job is repository-scoped and self-hosted.** Hardened,
  isolated roles on homelab-ai-1 own Linux validation, both Linux release
  targets, cargo-dist's global release phases under the `drep-linux` label,
  and mutation under the separate `drep-mutants` label. The Xcode-equipped M1
  at `Mac-Mini-3.local` owns native
  macOS validation and builds both `aarch64-apple-darwin` and
  `x86_64-apple-darwin` under `drep-macos`; the Xcode-free `.82` mini remains
  untouched. The validation workflow guards every job so forked pull requests
  are skipped before a LAN runner is selected, and cargo-dist uses
  `pr-run-mode = "skip"`, making the generated release workflow tag-only.
  `dist-workspace.toml` is the source of the release runner mapping; regenerate
  `.github/workflows/release.yml` with `dist init` rather than editing it.
  The `aarch64-unknown-linux-gnu` mapping must declare
  `host = "x86_64-unknown-linux-gnu"`: `drep-linux` is a custom label, so without
  the host cargo-dist assumes native arm64 and omits cargo-zigbuild and Zig.
  `.github/build-setup.yml` provisions pinned Zig 0.16.0 and cargo-zigbuild
  0.23.4 for that row before cargo-dist's generated pip fallback; homelab-ai-1's
  PEP 668-managed Python rejects the fallback's system install.
  The same setup provisions stable Rust and the matrix-selected target for Mac
  release builds, so they do not depend on a LaunchAgent or runner `.path`
  inheriting an interactive user's Rust installation.
  Cargo-dist's global homelab-ai-1 jobs still assume hosted-image tools: the
  runner `.path` must include its dedicated Cargo bin directory for cached
  `dist` and `/usr/local/bin` for `gh`. The role capability preflight must
  assert those entries and dist's version before admitting the runner; a
  service unit's own `Environment=PATH=` does not override the job PATH loaded
  from `.path`.
  Homebrew publication hardcodes the supported
  `/home/linuxbrew/.linuxbrew` prefix; the hardened service uses
  `ProtectHome=tmpfs`, `BindPaths=/home/linuxbrew/.linuxbrew`, and a matching
  `ReadWritePaths` entry so only that prefix is exposed while other homes stay
  hidden.
  Both validation toolchains install Clippy: the
  suite exercises a real Rust fixture whose compiler evidence is required to
  suppress a disproved LLM compile-failure claim. A previously provisioned
  runner may happen to have Clippy, but a fresh runner must not depend on that
  hidden host state or its unmutated baseline fails before testing any mutant.
  Every external Action in the hand-maintained Rust and build-setup YAML is
  pinned to a full upstream commit SHA with a
  version comment. `.github/dependabot.yml` updates workflow revisions but
  excludes cargo-dist's generated `release.yml`; build-setup lives outside
  `.github/workflows`, so the weekly security pass must check those pins
  directly. Validation explicitly requests only
  `contents: read`; do not rely on the repository's mutable default token
  permissions. `release.yml` is the deliberate exception because cargo-dist
  owns it and must remain byte-for-byte regenerable rather than hand-patched.
- **A release has two publication paths from one exact commit.** Bump the
  package version in `Cargo.toml` and `Cargo.lock`, move the complete
  `CHANGELOG.md` Unreleased material under that version and date, and update
  README's pre-commit revision together. An annotated `v<version>` tag drives
  cargo-dist's GitHub artifacts and Homebrew publication, and `publish-crate.yml`
  publishes the same tagged commit to crates.io, because cargo-dist does not
  publish Rust crates. Verify the tag target, GitHub assets,
  registry version and published Homebrew formula independently.
  cargo-dist 0.32 replaces the tap formula on every release and does not retain
  the manual audit fixes: it restores Cargo.toml's long article-leading
  description and redundant `version`, and removes the `test do` block. Run
  `brew audit --strict --online slb350/homebrew-tap/drep` after publication and
  restore the short description, inferred version and executable version smoke
  test in the tap before calling Homebrew complete.
  Version 3.0.0 is the language-coverage boundary: repositories in thirteen
  additional ecosystems changed from a successful no-op to recognized source
  analysis with their configured deterministic tools, so upgrading can add
  tool requirements or expose findings in repositories that previously passed
  without review.
- **A test that writes an executable uses `test_support::write_executable`**,
  which writes it from a child process. `fs::write` + `chmod +x` opens a write
  descriptor, `fork` copies the descriptor table, and Linux refuses to `exec` a
  file any process holds open for writing - so an unrelated test thread
  spawning a subprocess in that window makes the stub fail with `ETXTBSY`. One
  full-suite run in three failed on Linux this way while macOS stayed green.
  Inside a mutation run that is not a visible flake: it records a mutant as
  caught that nothing caught.
- **The mutation verdict comes from `missed.txt`, not from the exit code.** A
  *timeout* is not a failure: `i += 1` mutated to `i *= 1` never advances, and
  a suite that hangs has detected the mutant. But cargo-mutants returns exit 3
  (Timeout) *in preference to* exit 2 (FoundProblems) - `Outcome::exit_code`
  tests `timeout > 0` first - so a run with one hang and one genuine survivor
  also exits 3, and any script mapping 3 to success on the exit code alone
  waves that survivor through. Both callers used to carry a comment claiming
  cargo-mutants exits 0 for timeouts; it does not, and neither had met a
  timeout to find out. `--output` is passed for the same reason the verdict
  reads files: an unpinned `mutants.out` in the working directory is
  overwritten by a concurrent run in the same tree.
- **Every file under a `tests/` directory must be declared in that directory's
  `mod.rs`.** Rust silently ignores files no `mod` points at; four such files once
  held 31 tests that never compiled while the count looked right. Verify by
  appending invalid syntax and confirming the build fails.
- **`[lints] warnings = "deny"` lives in `Cargo.toml`**, not in CI flags or the
  hook, so a local `cargo clippy` agrees with both by construction.
- **API keys live in a user-level store, not in `drep.toml`.**
  `~/.config/drep/auth.toml` (macOS: `~/Library/Application Support/dev.slb350.drep`),
  mode 0600 in a 0700 directory, keyed by **endpoint** rather than by preset
  name - a key authenticates a host, and keying by preset would leave a
  hand-edited or custom endpoint unable to find its own credential. The endpoint
  is normalised (lowercased, trailing slash trimmed, path significant) so
  `https://API.Z.AI/v1/` and `https://api.z.ai/v1` are one entry, while
  `api.minimax.io/v1` and `api.minimax.io/anthropic/v1` stay two. `AuthStore`
  hand-writes `Debug` to print endpoints and never values, for the same reason
  `LlmConfig` and `LlmClient` do.
- **`auth::resolve` fills in only what the file left unset, and an explicit
  `api_key` always wins.** A user who wrote `api_key = "${VAR}"` said where the
  key comes from; preferring a stored one would make the file lie about what the
  run used, and would break CI, where the variable is the whole mechanism.
  Disabled entries are skipped, matching `${VAR}` expansion and field validation
  - looking a key up for a parked provider would report a missing credential for
  one that is never contacted. The returned `KeySource` list is *positional*,
  including skipped entries, because callers number providers by file position
  and by chain position and both index it.
- **`DREP_AUTH_PATH` relocates the store, and exists because nothing else
  could.** `directories` follows each platform's own convention rather than the
  XDG variables, so on macOS `XDG_CONFIG_HOME` is ignored and any command run to
  try something out writes into the real store - which is how a test key got
  into one. `init` and the `auth` subcommands also take the path as a parameter
  internally (`init::run_with`, `auth::run_at`) for the same reason
  `check::run_with` takes a root: a test using the real store reads the
  developer's own keys, which would make the rendered `drep.toml` depend on
  whose machine the suite ran on.
- **`drep init` is interactive when there is a person and no `--provider`.**
  `--provider` is the escape hatch that keeps every scripted invocation working,
  `--non-interactive` covers a script that wants the defaults without naming
  one, and `--interactive` forces the wizard where stdin is a pipe. The terminal
  check is load-bearing on its own: a hook or CI job has no stdin to answer
  with, and prompting there hangs the command rather than failing it.
- **The wizard decides; `init` acts.** `wizard::run` returns a `Plan` and writes
  nothing - no file, no key, no hook. `apply` carries it out through the same
  functions the flag path uses, so an answer given interactively cannot reach a
  different code path than the equivalent flag. Keys are stored *before* the
  config is written: a `drep.toml` naming no `api_key` is only correct once the
  store holds one.
- **Whether an existing `drep.toml` may be replaced is settled before the first
  question**, in `existing_config`. The ordering above is what makes this
  load-bearing: the wizard's own side effect is storing a pasted key, and that
  happens before the config write. Deciding afterwards meant a second
  `drep init` asked seven questions, saved a credential, and *then* failed on
  "already exists" - store changed, config not, provider not switched, exit 0.
  Interactively the current providers are printed and the answer defaults to
  *no*, because "Replace it?" is unanswerable without knowing what "it" is and
  Enter must not overwrite a working configuration; answering yes implies
  `--force`. Non-interactively it still refuses and names `--force`, unchanged,
  because a script has nobody to ask. `describe` is deliberately tolerant of an
  unreadable or unparseable file: it is describing something the user is about
  to discard, and erroring there would stop them replacing the very thing that
  is broken. It parses with `toml::from_str::<Value>`, never
  `str::parse::<Value>` - same type, different parser, and the latter reads a
  single TOML *value* and reported every valid config as unparseable.
- **Publishing `drep.toml` is atomic, including `--force`.** The rendered file
  is written and synced in a private sibling temporary file before it is
  persisted. The non-force path uses a no-clobber persist; the force path
  atomically replaces the directory entry. Opening the destination directly
  truncated a working config before the replacement was complete and followed
  a destination symlink into an unrelated file.
- **The wizard asks the endpoint which models it serves.** Every provider drep
  ships a preset for answers `GET {base_url}/models` with
  `{"data":[{"id":...}]}` whichever protocol it otherwise speaks; only the auth
  header differs, and only the `id` and an optional `display_name` are read, so
  a vendor adding a field cannot break the parse. This is why the key step
  precedes the model step - a listing needs authenticating, and listing with an
  empty key would 401 on exactly the endpoints the menu was built for. The
  alternative, a vendored catalogue, goes stale as the hardcoded defaults do
  *and* has to be noticed; models.dev describes what a vendor publishes rather
  than what this account's plan serves. That rejection is about *this* question
  only - a listing carries ids and nothing else, so the separate question of
  what a model accepts has no endpoint to ask (see the quirks registry below).
- **Nothing about a listing can stop `drep init`.** Every failure is reported
  and stepped past to the free-text prompt: an endpoint with no such route (a
  local llama.cpp build, a gateway) is the ordinary case, not a fault. A name
  outside the list is accepted too, because a model released this morning is
  exactly the one somebody is configuring - but a *number* outside it is
  re-asked, since that is a misread menu rather than a model called `9`. An
  empty listing is `Unsupported` rather than an empty menu. The endpoint's own
  order is preserved: all three list newest first, and sorting would bury it
  (`MiniMax-M2` above `MiniMax-M3`, `glm-4.7` above `glm-5.3`).
- **A pasted key is read without echoing, and the config then omits `api_key`
  entirely.** Writing `${VAR}` as well would name a variable the user never set,
  and an explicit `api_key` wins over the store - so the file would override the
  key `drep init` had just saved. The no-echo read falls back to a plain read
  when stdin is not a terminal: `rpassword` opens `/dev/tty` and fails outright
  without one, and piped input has no echo to suppress in the first place.
- **`.gitignore` handling asks git, never the file.** `git check-ignore` is what
  sees that `*.toml` already covers `drep.toml`, which no line-by-line
  comparison would. `git ls-files --error-unmatch` catches the state that makes
  a naive append useless: `.gitignore` has **no effect on a tracked file**, so
  appending there looks like it worked while `git status` keeps showing the
  file. That case is reported with `git rm --cached` rather than written. A file
  with no trailing newline is repaired first, or the entry joins the last line
  and both rules stop working - in a file drep did not write.
- **No `drep auth` subcommand prints a key.** `list` prints endpoints, `login`
  reads without echoing, `logout` reports only whether anything was removed. A
  `drep auth show` is deliberately absent: the store is a file, and anyone who
  needs the value can read it having chosen to.
- **A provider's wire protocol is a field, not an assumption.** `protocol =`
  selects `openai` (the default) or `anthropic`; `config::parse_protocol` is the
  single definition of what those names mean and it defers to the SDK's
  `ApiProtocol::from_wire`, so drep cannot come to disagree with the layer that
  acts on the answer. An absent value is the default, which keeps every file
  written before 2.1 valid; an *unrecognised* one is rejected at load, because
  defaulting would post chat-completions bytes to a `/messages` endpoint and the
  404 reads as "the provider is down" rather than "this line has a typo". A
  *disabled* entry's bad protocol is not fatal, for the same reason its unset
  `${VAR}` is not.
- **OpenAI API access and ChatGPT/Codex subscription access are different
  backends.** The `openai` preset remains the direct, API-key-authenticated
  `https://api.openai.com/v1` path; it is already supported and must not be
  relabelled as subscription usage. The `codex` preset is endpoint-less and
  keyless: it invokes a separately installed Codex CLI with
  `forced_login_method = "chatgpt"`, user/project config and rules ignored,
  drep-owned replacement instructions, every tool surface disabled, an empty
  cwd, an allowlisted environment, read-only/no-approval isolation, a strict
  output schema and ephemeral history. Codex owns login and token refresh;
  `drep auth --provider codex` points to `codex login` and never touches the
  endpoint-keyed store. `codex doctor --json` is reduced to version plus the
  ChatGPT-auth fact; a valid known document wins even if doctor exits nonzero
  for an unrelated check. The JSONL stream rejects tool activity and retains
  only bounded terminal diagnostics. Its JSONL line ceiling is enforced while
  reading, before an untrusted line can be allocated in full. Stdout and stderr
  are private bounded files rather than pipes: a tool-spawned grandchild can
  inherit a pipe and hold EOF open after Codex exits, whereas Codex's own exit
  is the end of the review. Capture size is monitored while it runs, and a
  stdin-write failure overrides only an otherwise successful exit so it cannot
  erase Codex's own failure diagnostic. The stdin writer is cancelled when the
  direct child exits: an inherited descriptor in a descendant must not turn a
  completed direct process into a timeout, while an incomplete payload still
  fails closed. Current terminal errors have no stable
  machine-readable class, so a nonzero exit stays `UnknownExit` and is never
  guessed from prose. HTTP and Codex cache identities differ by construction;
  Codex includes CLI version and reasoning effort. The default concurrency is
  one until live qualification proves a higher value is safe for plan usage.
  `docs/openai-integration-plan.md` records the measured choice of `codex exec`
  over app-server and the TDD/release gates.
- **`temperature` is `Option<f32>`, and absent means the parameter is not sent.**
  It defaulted to 0.2 while every endpoint drep could reach accepted it. Two of
  the four models drep now ships a preset for reject it outright - `k3` answers
  `only temperature 1 is allowed for this model` and `gpt-5.6-sol` refuses any
  value - and a 400 neither fails over nor retries, so "send none" had to become
  expressible rather than approximated by a low value. It is a property of the
  *model*, so the preset supplies a starting point, the quirks registry
  withdraws it for a model that refuses it, and `drep init` writes the line out;
  drep's own `drep.toml` carries `temperature = 0.2` explicitly for the same
  reason.
- **`max_tokens` stays unset except where the endpoint refuses a request
  without it, and the requirement is the endpoint's while the value is the
  model's.** An invented ceiling truncates a reasoning model mid-thought, which
  is the coupling 2.0 removed. `api.kimi.com/coding/v1` is the one exception
  found so far: it answers a bare `invalid_request_error` 400 that names no
  field. So `LlmPreset::max_tokens.is_some()` records the requirement, and the
  number it holds is only the fallback for a model the quirks registry cannot
  name - `kimi`'s 200,000, which the endpoint is verified to accept. For a model
  the registry *does* name, its own published output limit is written instead:
  131,072 for `k3`, 32,768 for `kimi-for-coding`. One provider-scoped number
  could not have been right for both. `b_presets.rs` asserts the exception list
  is exactly `["kimi"]`, so re-introducing a cap anywhere else fails there.
- **The quirks registry only ever narrows, and is keyed by endpoint.** `GET
  /models` answers which models a plan serves and says nothing about what any of
  them accepts, so `src/llm/quirks.rs` distils models.dev - the one index that
  publishes `temperature` and `limit.output` per model - into `endpoint -> model
  -> facts`. It may withdraw a `temperature` and may replace a *required*
  `max_tokens` with the model's own ceiling. It may never introduce either:
  sending a parameter drep would have omitted is the direction that produces a
  400, and omitting one it would have sent costs default sampling, so an index
  that disagrees with an endpoint cannot break a provider that worked before it
  existed. The join key is the provider's `api` URL through `auth::normalise`,
  never the model id - one open model is served under one name by a dozen hosts,
  which is the identity mistake `Provider::cache_key` exists to avoid - so a
  provider models.dev publishes with `api: null` (`openai`) simply never joins.
  `Quirks::max_tokens_from_registry` exists because the rendered comment claims
  the number is the model's own, and that claim lands in a file the user
  commits. It tests `limit <= fallback`, not `<`: the question is whether the
  registry named this model's limit, not whether it changed the value, and a
  model publishing exactly the fallback made the file say the limit was unknown
  while printing it. The `<=` is also what stops the flag being derivable from
  the two values, which is the reason it is carried rather than recomputed.
- **Nothing about model quirks can stop `drep init` either.** A missing,
  unreadable, unparseable or wrongly-shaped cache all mean "refetch"; a fetch
  that fails falls back to a stale cache if there is one and to the preset's own
  values if there is not; a model the registry does not name keeps the preset's
  values, which is what `drep init` wrote before any of this existed. The
  `--provider` flag path does not consult it at all - that path has no prompt,
  and a `drep init --provider local` that needed the internet would be a
  regression. `Registry::load` collapsing three failures into `None` is the
  opposite of `AuthStore::load`, which errors on a corrupt store: there the file
  holds something irreplaceable, here it holds a copy of a public document.
- **The model-quirks cache publishes through a random, exclusively created
  sibling.** A fixed `model-quirks.toml.tmp` opened with `fs::write` followed a
  planted sibling symlink and overwrote its target before the later rename;
  concurrent `drep init` runs also shared that one temporary. `NamedTempFile`
  gives each writer its own no-follow creation and atomically replaces only the
  canonical cache path.
- **The cache key includes the protocol as well as the endpoint.** One endpoint
  serves the same model over both wire formats - `api.minimax.io` publishes
  `/v1` and `/anthropic/v1` for `MiniMax-M3` - and the two are different requests
  with different reasoning handling. The same request identity includes
  `max_tokens` and the effective header set: a complete response under one
  ceiling remains a different bounded request, and an arbitrary header can
  select a tenant, route or feature variant. Header names are canonicalised
  case-insensitively and values remain exact hash inputs; values are never
  written into a cache path or entry as text. A credential rotation therefore
  cold-starts the cache rather than risking reuse across different backing
  behavior. An unset temperature keys as a distinct sentinel rather than
  folding onto a stand-in value, because omitting the parameter lets the server
  pick and the answers genuinely differ.
- **Interrupted response-cache writes still count toward the size ceiling.**
  Cache temporaries use the reserved `.drep-cache-tmp-` prefix inside a valid
  shard, and eviction recognizes that prefix alongside complete digest entries.
  It must continue to ignore every other file, including foreign JSON placed in
  a shard; broadening the walk turns a cache-size cleanup into user-data loss.
- **An absent `api_key` means no protocol authentication header.**
  `LlmClient::new` passes the SDK an empty key, which suppresses both OpenAI
  `Authorization` and Anthropic `x-api-key`; inventing `not-needed` breaks a
  gateway whose configured header is the complete authentication scheme.
  `doctor` therefore labels this source as the protocol key and, when custom
  headers exist, does not prescribe `drep auth login` as though authentication
  were necessarily missing.
- **The whole response is parsed before any fence is looked for.** A model
  reviewing code that discusses code fences puts "```" inside a finding's
  message, so a response that is valid JSON from the first character still
  contains a fence - and a fence-first ladder replaced the working text with
  that inner fence's body, failing every later strategy on prose. drep's own
  gate hit this on `json_parsing.rs`. Trying the full content first cannot
  mis-fire: if it parses, it is the answer.
- **The credential store never widens or narrows a directory drep did not
  create**, because `DREP_AUTH_PATH` can name any path and chmodding its parent
  would let `/etc/drep.toml` turn `/etc` into 0700. `auth::ensure_dir_private`
  is the single definition of that rule, shared with the quirks cache because
  the two files share a directory - a second copy that only called
  `create_dir_all` would leave the store's own directory world-readable whenever
  `drep init` happened to cache first. Each update is written through a random,
  exclusively created 0600 sibling and atomically persisted: a predictable
  sibling opened with truncation can follow an attacker-planted symlink and
  write the complete credential store into its target before publication. A key
  carrying a control character is refused at the prompt rather than failing as
  a header on the first request. Each login or logout is journaled and replayed
  over a fresh read while holding both the process mutex and the persistent
  sibling file lock; serializing stale snapshots directly loses unrelated
  concurrent updates and can resurrect a key another process removed.
- **`api_key_command` has one bounded, secret-safe execution contract.** It is
  an argv, never a shell line; it runs once per enabled HTTP entry before the
  chain exists; and a failure is fatal rather than falling through and hiding a
  broken credential path. The whole trimmed stdout is the credential, capped at
  64 KiB, while stderr and captured output never enter either `Display` or
  `Debug`. Environment-reference errors must not echo the argv element either:
  an argument is allowed to carry the secret directly. The configured direct
  child's exit is decisive even if a background grandchild inherited stdout;
  waiting for pipe EOF instead let that unrelated descendant turn a successful
  helper into a timeout. The biased stdout/exit select plus bounded final drain
  retains the direct child's final write without waiting for the descendant to
  close the pipe.
- **Site policy fails closed at every filesystem boundary.** The installed
  policy path may be displaced by `DREP_SITE_CONFIG` only when
  `symlink_metadata` returns `NotFound`; another metadata result keeps the
  machine path authoritative, and a dangling policy symlink is therefore a
  fatal read error rather than no policy. Marker probes likewise treat only
  `NotFound` as absence; any other inspection error is fatal. Both
  `refuse_markers` and `max_concurrent_ceiling` are rejected in `drep.toml`,
  because silently dropping a site-only spelling creates a false claim of
  enforcement. Policy scope follows the bytes semantic review sends: paths
  mode canonicalizes a named source symlink, reads from that target and probes
  that target's repository; diff modes retain hunk parents because deleted
  paths cannot be canonicalized. Those directories are collected only when the
  loaded policy names markers, and their bounded ordered Git-resolution stream
  returns on the first refusal or error rather than waiting for every probe. A
  refusal neither constructs provider state nor reads, creates, writes or evicts
  the cache, and `doctor` must reach the same answer from its discovered source
  directories without opening the auth store, running a helper or probing
  Codex.
- **`SiteConfig::has_refuse_markers` is a load-bearing preflight, even though
  `refusal_among` also guards an empty marker list.** The outer check prevents
  callers from collecting source directories and attempting Git resolution
  for a ceiling-only policy; a direct test pins both its false and true branches
  because end-to-end refusal tests cannot distinguish a broken preflight from
  the inner guard.
- **`auth::normalise` lowercases the scheme and host but never the path.** Paths
  are case-sensitive; collapsing them merged two endpoints on one host into a
  single entry and handed one's key to the other.
- **No test writes to the process environment.** `std::env::set_var` is `unsafe`
  in edition 2024 because a concurrent reader on another thread is a data race,
  and `cargo test` is multi-threaded - so a "single-threaded test process"
  safety comment is false. `auth::path_from` takes the override and the wizard
  takes an `env_is_set` lookup, so both are testable without touching the
  process. `quirks::path_from` and `quirks::Cached::at` are split from their
  environment- and clock-reading callers for the same reason. The remaining
  calls are in `config/tests/env.rs`, which tests `${VAR}` expansion itself and
  saves and restores each value.
- **There is no `load_default`/`save_default` on the store.** Both were wrappers
  over a path read from the environment, so nothing could test them and the
  mutation gate found them undetectable. `check`, `init`, `auth` and `doctor`
  each resolve the path at their own entry point and pass it down, which is also
  what keeps the suite off the developer's real keys. The quirks cache follows
  the same rule: `init::run_with` builds `quirks::Cached` in the interactive
  branch alone, which is what keeps every test of the flag path off the network.
- **A `cfg`-gated twin of a function is invisible to the mutation gate**, since
  the copy that is not compiled cannot be observed and reports as a survivor on
  every run. Put the `cfg` around the smallest differing expression instead -
  `write_private` gates only the `mode` call, and `create_private_dir` gates the
  platform-specific body, rather than duplicating either whole function.
- **`json_parsing::strip_reasoning_block` runs before the extraction ladder.**
  Reasoning models are supposed to stream deliberation on a side channel, which
  the SDK routes away from content; several OpenAI-compatible servers instead
  emit the whole trace inline at the head of `message.content` in `<think>`
  tags. Deliberation about a code review quotes code, so that trace carries a
  fenced block of its own, and `FENCE_RE` takes the *first* fence - the ladder
  selected the reasoning's sample, every later strategy failed on it, and the
  file came back `Unparseable`, which neither fails over nor retries. The strip
  is anchored at the start (a `<think>` anywhere else is content, most obviously
  a finding about a file containing the tag) and requires a closing tag (without
  one the answer never arrived, and `Unparseable` is then honest).
- **Config declares providers as `[[llm]]`, an array of tables**, and it is the
  failover chain: `Config::providers()` returns the enabled entries in file
  order, and `src/llm/chain.rs` tries them in turn. A file declaring none is
  rejected by `config::load` (`NoProviders`); one where every entry is disabled
  is rejected too (`NoEnabledProviders`), because neither can ever produce a
  passing run and the file is what should be named.
- **`enabled` is an opt-out and defaults to `true`.** A disabled entry is
  skipped wherever it sits, so parking the local model falls through to the
  cloud entry below instead of failing the run. It defaulted to `false` while
  only one entry was consulted, which meant declaring a provider did nothing
  until you also enabled it - a fallback copied from the first block, minus its
  `enabled` line, was silently inert.
- **"Does the chain advance" and "is the failure remembered" are separate
  questions.** `should_failover` answers the first: status-less failures
  (timeout, refused connection, empty body) and 408/429/5xx advance; a 401/403
  stops the chain, because that is misconfiguration and falling back masks it,
  while a non-empty unparseable body advances after three response attempts.
  That parse failure is request-specific and is therefore explicitly excluded
  from `is_sticky`: a fallback may salvage this file, but the primary must be
  tried normally for the next one.
  `is_sticky` answers the second and covers a *wider* set - every endpoint-level
  failure, 401 included, because a stale key answers the same way for every file
  and re-handshaking once per file is pure wall-clock on the gate. A remembered
  failure is replayed through `should_failover`, so a demoted 401 still stops
  the chain rather than being silently routed around. `is_sticky` is
  `should_failover(err) || is_auth_failure(err)`, never "every transport
  failure": remembering a failure that does *not* fail over stops the chain for
  every later file, so one oversized payload drawing a 400 poisoned the whole
  run. Only 401/403 earn that, being properties of the connection rather than
  the request.
- **The cache key includes the endpoint, not just the model**, and is computed
  inside the failover loop via `Provider::cache_key` - the single definition of
  which key belongs to which provider. A model name is not an identity: one
  open model served locally and from a cloud provider is the canonical failover
  pair and both name it the same thing, so keying on the model alone files the
  fallback's answer where the head looks for its own. Keying the head and
  letting the fallback serve has the same effect. A test that mounts a dead A
  and a healthy B proves only that the loop advanced; so does one that varies
  the model name. Assert on the *key*, through `Provider::cache_key`, and cover
  the same-model case.
- **Cache eviction runs after a completed check and owns only canonical entries.**
  Concurrent analyzers finish before the size pass, so a successful run cannot
  leave the cache permanently above its documented ceiling. Eviction considers
  only regular files named as the exact lowercase 64-hex digest in their
  matching shard; a user-created `notes.json`, symlink, or malformed entry in
  the cache directory is never counted or deleted.
- **Disabled providers are inert, not merely unselected.** `${VAR}` expansion
  and field validation both skip them, so parking the cloud entry does not
  refuse to load the file because its `${OPENROUTER_API_KEY}` is unset. An
  *enabled* entry's unset variable is still fatal.
- **`max_concurrent = 0` is rejected at load.** A semaphore with no permits
  never hands one out, so the gate would hang with no message rather than fail.
- **Numeric request controls that cannot do useful work are rejected at load.**
  `timeout_secs = 0` expires every request immediately and `max_tokens = 0`
  cannot produce a review. Environment placeholders must name a non-empty
  variable and distinguish an unset variable from a non-Unicode value; doctor
  uses the same distinction instead of calling the latter "unset".
- **`LlmConfig` hand-writes `Debug` to redact `api_key`**, for the same reason
  `LlmClient` does. `Config` derives `Debug` and holds them, so a derived one
  meant any `{:?}` on a loaded config emitted a live credential.
- **Provider demotion is sticky for the run, and re-checked after the limiter.**
  The failure classes that fail over are endpoint-level, so one dead endpoint
  must not cost every file the SDK's full backoff schedule. Files run
  concurrently, so the check before acquiring a slot only stops files that had
  not started; without the second check after the slot is granted, everything
  already queued still pays. A provider's content-specific cache is consulted
  before demotion: serving its existing verdict spends no request, while
  skipping it would route around the configured preference for no benefit.
- **A one-provider *chain* collapses to that provider's own `FailureReason`.**
  `FailureReason::ChainFailed` appears for any chain of two or more, so a
  one-provider config - what `drep init` writes - reports exactly what it did
  before failover existed, JSON `kind` included. The trigger is the chain's
  length, never the number of providers that failed: those differ exactly where
  it matters, because a 401 at the head of a two-provider chain produces one
  attempt and is the case where "which provider, and why didn't my fallback
  run" is the live question.
- **Provider numbering is one-based chain position, everywhere a user sees it.**
  `doctor` numbers only the enabled entries and bullets the disabled ones, so
  its `1.` is the same provider the failure line calls `[1]`. `ConfigError`
  numbers the *file* instead and says "in file order" out loud, because
  `[[llm]] #1` meaning two different tables in one file is worse than either
  convention alone.
- **Two size ceilings, deliberately separate.**
  `analysis::payload::PAYLOAD_MAX_BYTES` is checked against the *rendered
  payload* in `code_quality::analyze_file`, so it holds for every input mode,
  and it is the authority on "too large to analyze".
  `cli::check::input::READ_MAX_BYTES` only stops `read_to_string` pulling a
  pathological file into memory in paths mode. One constant for both made the
  reported byte count depend on which path measured it, hence the split
  `FailureReason::FileTooLarge` / `PayloadTooLarge`. A `const` assertion pins
  that the read guard never sits below the payload ceiling. A file too large
  for the model is still linted, via `Work::lint_only` — it has a path, and
  ruff reads the file itself.
- **`crate::http` is the only GET drep issues for itself, and the only bound on
  a body it did not produce.** Reviews go through open-agent-sdk; what is left
  is `drep init` asking an endpoint which models it serves and asking
  models.dev what those models accept. Both are one request against a host the
  user named. The ceiling lived in the registry fetcher alone while the listing
  call next to it buffered `text()` unbounded - a safety property written twice
  is written once and forgotten once. Classification is deliberately *not*
  shared: a 404 is an ordinary answer for a listing and a fault for the
  registry, so each caller keeps its own error enum and maps `ReadError` into
  it. `reqwest` carries the `gzip` feature because models.dev serves 399 KB
  compressed against 4.01 MB, which on a slow link is the difference between an
  invisible refresh and a stall inside the 20-second timeout; reqwest strips
  `Content-Length` from a response it decodes, so the header check is a
  shortcut and the per-chunk cap is the guarantee. That cap counts *decoded*
  bytes, which is both what gets allocated and what makes a body that inflates
  without limit refusable.
- **`finish_reason` decides whether asking again can help; the body never did.**
  open-agent-sdk yields `StreamEvent`, ending in exactly one
  `Finish(FinishReason)`. `Length` and `ContentFilter` are *request*-shaped: the
  same request hits the same cap, and a filter that refused this payload refuses
  it again. They end the attempt as `LlmError::ModelStopped`, which - like a 400
  - must never fail over and must never demote the provider, because a second
  provider cannot make the file smaller. Everything else (including
  `Unspecified`, which several OpenAI-compatible servers always return and which
  is *not* `Stop`) stays retryable. This classification also applies when the
  content is empty: a terminal finish reason is more specific than the generic
  empty-response transport rule below.
- **Three outcomes, three rules, and the middle one is not "deterministic".**
  An *empty* response is a **transport** failure: zero characters means the
  provider did not answer, retrying costs almost nothing because no output
  tokens were produced, and it may fail over. A response that parsed only after
  brace-balancing (`Extracted::Truncated`) is the genuinely deterministic case -
  the same prompt is cut at the same place, and it already yields a usable
  partial - so it is never retried. A response with **no JSON at all** sits
  between them: it is retried up to `NO_JSON_ATTEMPTS`, but stays
  `Unparseable` and never becomes `Transport`, because prose says nothing about
  the endpoint and `Transport` would fail over *and* demote the provider for
  the whole run.
- **The no-JSON retry lives in `complete_json`, not in the SDK's retry layer.**
  Handing it to the SDK by returning `Err` would work, and would then surface
  as `Transport` once attempts ran out - which is exactly the misclassification
  above. The SDK's retry still runs inside each pass, so transport failures are
  handled by the layer that classifies them.
- **`LlmError::Unparseable` carries an excerpt of the body.** It was a constant
  string, so every occurrence looked identical and there was no way to tell a
  refusal from a prose preamble from reasoning that leaked into the content
  channel. The excerpt is bounded and control characters are stripped - it is
  model output, and it lands in a terminal.

  The "never retry a non-empty body" rule these replaced was reaching for the
  `Length` case above and identified it by the wrong proxy - the shape of the
  body rather than the server's own reason - so it borrowed truncation's
  justification for a case truncation does not cover. Repeated gated runs
  showed different files failing each time, and each analyzed cleanly when
  asked again. The empty-vs-non-empty split was verified the same way.
- **`cargo clippy` takes no file arguments** (`ToolSpec::accepts_files =
  false`). It checks a crate and rejects a path outright, so appending files
  makes every Rust run `Unavailable`. A whole-project tool is invoked bare and
  its findings are narrowed to the files
  being checked, or a commit gate blocks on pre-existing issues elsewhere.
  Stub-based `run_tool` tests cannot see this class of bug: only the real
  binary can say whether it accepts the argv drep builds.
- **`tsc` also takes no file arguments from drep.** Passing a source path makes
  TypeScript ignore `tsconfig.json`, so it runs the configured project and its
  diagnostics are narrowed back to the requested files just like clippy.
- **A non-zero exit with zero parsed findings is `Unavailable` whenever the
  parser skips input it does not recognise.** The position/tsc/MSBuild parsers
  skip by design, so a run whose every diagnostic is of a shape they do not
  know - MSBuild's position-less `x.csproj : error NETSDK1004`, a rejected tsc
  option - parses as zero findings on a non-empty stream and read as a clean
  pass. The empty-stream guard cannot see it, so `run_tool_at` carries a
  second arm keyed on `OutputFormat::skips_unmatched_input`, with the
  unmatched lines as the detail. JSON-shaped formats error on such output
  already; adding them to the skip set would report a legitimately empty
  clean run as `Unavailable`, which is why the set is exactly the three.
- **A SARIF result with no usable location is `Unavailable`, not a finding.**
  tflint reports runtime errors (plugins never installed, arguments it
  dropped) as a `tflint-errors` run whose results carry no `locations`; read
  as a finding, the empty path matched nothing the run was asked about and
  narrowing dropped it, so a tflint that never examined a file reported every
  Terraform file clean. The parser rejects such a result with the tool's own
  message as the detail. Keying on the location rather than the run name
  matters because a healthy tflint run also emits a `tflint-errors` run - an
  empty one - and because a `physicalLocation` with no `region` is a real
  finding (it defaults to line 1), not this error.
- **`output_format` and `diagnostics_stream` are fieldless enums.** A typo'd
  format string failed at runtime only when the tool ran, and a typo'd stream
  silently selected stdout; the dispatch match and the stream selection are
  now exhaustive at compile time. No `#[non_exhaustive]`: this crate is the
  only consumer, and the wildcard arm reintroduces the hole. Registry data is
  validated by `every_registered_entry_is_well_formed` over `ALL_LANGUAGES`
  (dot-prefixed ASCII extensions, non-empty command, non-zero timeout, no
  `config_flag` without `config_files`, single-component `vendored_dirs`),
  because none of those contracts is visible to the type system and each was
  once broken silently. `ALL_LANGUAGES` itself is concatenated from each
  ecosystem file's `FAMILY` slice: a static could be defined and re-exported
  while never being registered, invisible to every test.
- **`filename_prefixes` claims a name's dotted variants; `filenames` claims
  the exact name.** `Dockerfile.dev`/`Dockerfile.prod` are an unbounded
  family an exact list cannot cover, and an unclaimed one is dropped from
  walk, staged and diff modes entirely - the same silent pass the
  whole-name lookup exists to refuse. Extension lookup runs before both
  name rules, so `Dockerfile.ts` stays TypeScript; a stem must be followed
  by a dot and a non-empty variant.
- **cppcheck runs with `--error-exitcode=2`, tflint with `--recursive`.**
  cppcheck otherwise exits 0 *with* findings, leaving the exit-status guard
  inoperative for it: a release that moved its SARIF stream would read as a
  permanent clean pass. Bare tflint lints only the module in its cwd, so a
  commit touching `modules/*/` produced no findings and passed silently;
  each module's config is its own (a nested module without `.tflint.hcl`
  gets tflint's defaults), which is tflint's documented per-module
  resolution, and the recursive run's `../..`-carrying uris under a
  symlinked checkout are why the canonical comparison above exists.
- **checkstyle cannot run bare, and ktlint cannot run without
  `--log-level=none`.** checkstyle exits 1 with "Must specify a config XML",
  so `ToolSpec::config_flag` hands it the ruleset `config_files` discovered,
  appended ahead of the file arguments; ktlint writes logging to *stdout*
  ahead of the reporter's JSON, which the parser would reject as unparseable.
  ktlint's own sarif reporter is not an alternative to the `ktlint` format: it
  binds `%SRCROOT%` to the process home rather than the working directory, so
  its relative finding URIs resolve to nothing drep was asked to check.
- **A `file:` URI is decoded completely inside `languages::runner::uri`, and
  the decoding is the URI spec's rather than any tool's.** SARIF is the only
  format that mandates a URI, so `strip_file_uri` is the single definition of
  turning one into a path: it removes the scheme, percent-decodes the body
  (checkstyle's SarifLogger encodes space and quote as `%20`/`%22`), and drops
  the root slash RFC 8089 requires ahead of a Windows drive letter. Each step
  is load-bearing for the same reason - `check` rewrites a finding back to the
  path the user asked about by looking its absolute path up in a table, and a
  half-decoded path matches nothing there, so the finding keeps a location no
  file has and is silently dropped. The drive rule runs on every target rather
  than under `cfg(windows)`, because a `cfg`-gated twin is invisible to the
  mutation gate and no Unix producer emits a first component that is a bare
  ASCII letter followed by a colon. It is written as four named conditions
  rather than one slice pattern for the reason `percent_decode` writes
  `hi * 16 + lo`: cargo-mutants does not mutate match patterns, so the
  `matches!` form states the same rule while reporting zero viable mutants
  against this form's five. Normalisation splits here and at the matching
  layer: parsers decode their own format, and `runner::narrow::joined_reported`
  is the single definition of resolving a reported path - both
  `retain_requested` and the `run_one` rewrite compare through it, in exact
  form and then canonical form, because a tool deriving paths from its
  resolved cwd (a symlinked checkout, or tflint `--recursive`'s
  `../..`-carrying relatives) spells them differently from drep and a
  byte-exact match drops every finding. The canonical form is resolved only
  after the exact form has missed, and each side's index of it is built on
  that first miss: every member costs a `realpath`, and under an ordinary
  checkout nothing ever misses, so resolving up front spent one syscall per
  finding and per planned file to answer a question nothing asked.
- **Deterministic tools run from the nearest configured ancestor of each
  changed file.** This is the monorepo boundary: batches are keyed by tool and
  workspace root, while executable resolution walks from that workspace to the
  repository root so npm-hoisted binaries still work. At most four ordinary
  tool processes start together, and clippy tasks are serialized within the
  repository so workspace fan-out does not manufacture contention on the same
  Cargo build lock. Looking only at the repository root left nested
  eslint/tsconfig projects as LLM-only coverage.
- **Clippy's process ceiling is 1,800 seconds; other tools keep 120.** Cargo
  waits for its build-directory lock inside the child process, so applying the
  generic timeout made an unrelated build turn the whole review into exit 2.
  If the extended ceiling is exhausted, the diagnostic names the lock wait.
- **Compile-error claims are structured and compiler-grounded.** The LLM schema
  carries `compile_failure`; a zero-exit clippy, tsc or go-vet run marks the
  exact files it compiled, and only matching LLM compile-failure claims are
  suppressed. Do not infer this from message prose or suppress semantic
  findings a compiler cannot decide.
- **Acknowledgements are source-sensitive and repository state.** LLM findings
  receive a fingerprint of file, category and nearby source. `drep acknowledge`
  atomically writes `.drep/acknowledgements.toml`; checks only read it. Line
  numbers are excluded so a pure shift stays acknowledged, while a nearby code
  edit changes the hash and expires the decision.
- **Semantic review is high-signal, not exhaustive.** The prompt asks only for
  concrete, reachable, materially consequential defects worth fixing before
  merge and explicitly excludes optional hardening, implausible extreme edge
  cases, nits, cleanup and speculative findings. The prompt and strict output
  schema expose only critical, high and medium; the parser still accepts low
  and info so an older cache entry or an unconstrained provider response cannot
  turn the whole file into an analysis failure.
- **Fresh semantic remediation has a three-round default, enforced fail-closed.**
  `--staged`, `--diff`, `--pre-commit-push` and a bare `--push-gate` are the
  authoritative scopes; named paths are deliberately uncounted. Each cold
  provider pass atomically reserves a worktree-and-branch-local slot before it
  starts, so concurrent checks cannot oversubscribe the limit. A shared
  advisory lock serializes claim, commit, refund and reset across branch
  identities; each lease carries an owner token that must still match before a
  process may commit or refund it, so a stale owner cannot alter a successor's
  slot. The slot is committed only when the fresh result still has an
  actionable finding after
  compile-claim suppression and acknowledgements; a clean answer or pure
  analysis failure refunds it, while a mixed finding and failure consumes it.
  Once a slot is reserved, selected misses bypass cache so a concurrent cache
  write cannot be relabelled as this process's fresh response.
  Cached verdicts and deterministic tools remain available at the limit, but a
  cold miss becomes `ReviewLimit` and exits 2 without contacting a provider.
  A clean complete diff, pre-commit-push or bare push-gate check resets committed
  rounds; staged subsets and named paths cannot erase full-branch state. Reset
  preserves another process's pending lease and removes empty branch state.
  Claim, commit and reset I/O failures are fatal because they are authoritative
  quota transitions; only response-cache maintenance is best-effort. A
  pending or incomplete lease is recoverable only after seven days so a killed
  writer cannot exhaust the
  budget forever or immediately reopen it. `max_review_rounds` defaults to 3
  and rejects zero. Agent-run checks must preserve that ceiling: never pass
  `--unlimited-reviews`, and never set `--max-review-rounds` or
  `max_review_rounds` above 3. When the three rounds are exhausted, stop and
  hand the remaining findings to the user instead of bypassing the limit.
  Review-cycle state lives under the current worktree's Git directory, never
  in the content-addressed response cache.
- **The published pre-commit pre-push hook resolves refs, not filenames.**
  `--pre-commit-push` reads pre-commit's FROM/TO environment and hands the
  exact range to `hunks_between`; `pass_filenames: false` is load-bearing,
  because a filename selects whole-file mode and makes each small repair
  reopen unchanged code for review. Pre-commit's ref-less all-files signal for
  a new root branch stays all-files, while missing or partial hook context
  fails closed.
- **`drep init` writes native git hooks, not a pre-commit entry**, and the
  hooks directory comes from `git rev-parse --git-common-dir` (in a linked
  worktree `.git` is a *file*). A set `core.hooksPath` makes git ignore
  `.git/hooks` entirely, so a chainer in that directory is what keeps the
  repo-local hook alive; an *empty* `core.hooksPath` means hooks are disabled,
  not "hooks live in the cwd". If the configured path resolves to the same
  repository hooks directory, the native hook is already active and no chainer
  may be written there; such a chainer would overwrite the hook and exec itself.
- **The pre-push hook passes `--tip`.** `--diff <base>` is
  `git diff <base>...HEAD`, and the ref being pushed is not always the
  checked-out one - without the tip, the pushed branch is never reviewed.
- **The pre-push hook uses `--push-gate`, and exit 3 means “reconnect”.** Git
  opens the remote transport before invoking `pre-push`; a cold LLM review can
  leave it idle long enough for the remote to close it. The gate therefore
  tries every provider cache without contacting a backend. A warm verdict
  continues immediately. A miss is reviewed and cached in the foreground,
  then deliberately exits 3 and asks for another `git push`; the retry opens a
  fresh transport and is cache-only. A failed warm or blocking finding keeps
  exit 2 or 1 and never masquerades as the reconnect handshake. Do not move
  the review into an unobserved background process: the first push must know
  whether caching actually succeeded. Across multiple pushed refs, hook
  precedence is semantic (`2 > 1 > 3 > 0`), not numeric: reconnect status 3
  represents a successful review and must not hide a harder failure.
  Every other nonzero status fails closed as 2; a panic, signal or future exit
  code must never fall through the shell `case` and wave a push through.
- **A hook is drep-managed only when its marker is structural.** The marker is
  the first line, or the first line after a shebang; merely mentioning it in a
  foreign hook's comments does not authorize replacement. Hook installation
  resolves and validates the active `core.hooksPath` before writing the local
  hook, refreshes an outdated managed chainer, and accepts a foreign chainer
  only when the executable word of an active command names the repository
  hook. A comment, `echo`, conditional test or other later argument that merely
  names `hooks/<name>` is not forwarding and must not make installation report
  success while the gate is bypassed. This also prevents a comment from turning
  user-owned executable code into replaceable state. Executable-word parsing
  accepts balanced single or double quotes, escaped spaces and nested command
  substitutions, but rejects unterminated quotes, substitutions and escapes;
  an escaped dollar never opens a substitution. A malformed shell fragment must
  not be credited as a working gate.
  Foreign hooks are read as bytes, and `--force` backs those bytes up through a
  no-clobber publish; invalid UTF-8 must not block installation or be changed,
  and an older recovery copy must never be overwritten.
- **Cache publication is atomic and never follows the destination.** Each JSON
  value is written to a unique temporary file inside its shard and persisted by
  replacing the canonical path. Writing the canonical path directly exposes
  partial JSON to concurrent readers and follows a planted symlink; a shared
  fixed temporary name also lets concurrent writers corrupt one another.
- **`config::env_var_refs_in` is the single definition of a `${VAR}`
  reference**, shared by the substituter and by `doctor`. A narrower scanner in
  `doctor` made it report a config as fine that `check` refused to load.
- **The LLM payload carries true file line numbers, and a finding outside
  `Payload::valid_lines` is dropped.** `src/analysis/payload.rs` renders each
  line as `{marker}{n:>6} |` followed by a space, with a blank number for
  removed lines. The model
  is never asked to derive a line number from an `@@` header — that is how a
  finding ends up pointing at plausible-looking wrong code. A finding on a line
  that was never sent is dropped, never clamped to the nearest one.
- **Analyzer input is partitioned by path even when a caller mixes files.** The
  ordinary diff path already groups hunks, but `analyze_file` is public and a
  debug-only assertion does not protect release builds. Mixed input is split
  and merged so no finding can be attributed to the first hunk's file.
- **Tests use the narrowest existing boundary that establishes the contract.**
  Render fixtures with `render_to`; keep focused subprocess checks for CLI
  wiring. Pure cache-key comparisons need no server or filesystem writes.
  Recovery tests must retain the failed endpoint's identity. Remove superseded
  duplicate assertions while preserving distinct failure, security and
  concurrency cases. Check user-visible errors through their formatted
  diagnostic. A fixture exercising one validation check must satisfy the other
  checks, so they cannot hide a broken one. Aging fixtures set file times
  explicitly; reservation ownership cases distinguish tokens from timestamps.
  Timeout fixtures exercise actual polling and child termination, with fallback
  cleanup before assertions; the production Codex diagnostic deadline stays
  30 seconds while its internal runner accepts an explicit test deadline.
  Comments state current contracts
  and non-obvious reasons; development history belongs in the changelog.
- **Process-boundary fixtures prove ordering causally, never by elapsed time.** A test about which of two processes finishes first makes the order observable (a fixture that holds a pipe until the other process has been reaped, then drains it) instead of timing the call against a deadline, which a loaded suite exceeds. A shell fixture hands a pipe to a background command on another descriptor, because dash, `/bin/sh` on the Linux runners, gives a background command `/dev/null` as stdin.
- **Cross-module fixtures live in `src/test_support.rs`.** Reuse its SSE,
  retry and model-registry builders; `fast_retry_client` must not override
  `max_attempts`. Integration tests are separate crates and share
  `tests/common/mod.rs`. Module-local argument and outcome builders centralize
  defaults so cases specify only the fields relevant to their scenario.
- **wiremock cannot demonstrate concurrency.** It runs `respond_with` under its
  own state lock and never overlaps requests: measured, four requests take
  652 ms at `max_concurrent = 1` and 595 ms at `max_concurrent = 8`. Neither an
  in-flight counter nor wall-clock can tell a working limiter from a deleted
  one. Observe `Limiter::available()` instead.
- **`check` and `lint-docs` own disjoint file classes.**
  `files::is_scan_target` is registered-language sources; `files::is_markdown`
  is markdown; nothing satisfies both. `is_scan_target` delegates to
  `languages::detect`, the single allocation-free registry lookup, so the
  walker's answer cannot drift from the language the analyzer will actually
  use. `is_scan_target` used to accept `.md`
  while no language claimed it, so `drep check README.md` read the file, found
  no deterministic tool and no LLM language, and printed "No issues found." A
  file drep declined to analyze, reported as clean, on a path the user typed.
  Any explicitly named file the running command's predicate rejects is now
  `FailureReason::Unsupported` (JSON `kind: "unsupported"`), carrying the
  extension and what to run instead; `.txt` had the identical bug. A *walk*
  that finds nothing analyzable is still legitimately empty, which is the
  distinction `resolve_paths` already drew for a non-existent argument.
- **`files::expand_named` is the single answer to "what did the user ask for,
  and what could I not do with it".** It returns `Expansion { targets, rejected }`
  and defaults empty arguments to `root` without treating it as explicitly named.
  Both expansion APIs share one classification pass: one `fs::metadata` per
  named path, and one predicate call per named regular file. Collect targets
  and rejections from that decision; never classify and then expand the same
  paths again. Named special files must remain rejected, while explicitly
  named regular files bypass gitignore and directory walks honor it.
- **`files::owning_command` is the one table mapping a path to the command
  that analyzes it**, and `redirect_hint` is the one phrasing of "run `drep X`
  instead". Each command used to hardcode a pointer at the other, asking the
  question two different ways (`is_markdown` one way, `languages::detect` the
  other) - an O(n²) table with no owner, where adding a file class means
  editing every existing command and forgetting one yields a hint-less dead
  end.
- **`crate::text::excerpt` is the only bounding of text drep did not write.**
  Model responses and URLs copied out of markdown both land in a terminal, and
  both must have control characters replaced or an escape sequence in them is
  interpreted. A second copy written for `bare_url` truncated but did not
  strip, which is the one thing the function exists for.
- **`cli::render` owns the finding line and the "could not be analyzed"
  block**, shared by `check` and `lint-docs`. The source prefix (`tool/`,
  `llm/`) is a parameter, which is the one deliberate difference; `lint-docs`
  passes `None` because it has a single source. They were transcribed copies,
  so `lint-docs` inherited by luck the rule that the blank separator above the
  failure block appears only when findings precede it - a fixed bug.
  Routing markdown into `check` was the rejected alternative: it needs a third
  gating category, because the doc checks are deterministic but they are
  drep's opinion rather than the project's configured tool, and it has no
  answer in the diff modes where a whole-file check like `unclosed_code_fence`
  would see an odd delimiter count on every partial view.
- **`docs::fence::Fences` is the single answer to "is this line inside a code
  fence"**, derived once per file, and a delimiter line counts as inside one.
  A check that tracks fence state itself is the bug. Which checks consult it
  is decided by one question: would this check's advice be wrong inside a
  fence? Headings, `long_line` and the link checks, yes (`#!/bin/bash` in a
  bash sample is a shebang). `tab_character`, yes, because "replace tabs with
  spaces" stops a ```` ```make ```` sample being a
  Makefile. `trailing_whitespace`, no, so it fires everywhere. The fence table
  in `src/docs/tests/fence.rs` asserts its own completeness, so a new check
  cannot skip the decision.
- **Doc-check severity is "does it change how the document renders".** An
  unclosed fence turns every line below it into code, so it alone is `error`;
  a heading or link that renders wrong is `warning`; whitespace and line
  length render identically, so they are `info`. That is what keeps
  `lint-docs --strict` calibratable. On a scale where a trailing space blocks
  a commit, the gate gets switched off.
- **`lint-docs` gates through `--fail-on <severity>`, and `--strict` is the
  shorthand for `--fail-on info`.** One threshold, resolved once in
  `LintDocsArgs::threshold`, so the gate never learns that two flags exist. The
  installed hook and the published one both ship `--fail-on error`: under this
  severity scale `--strict` blocks on any finding, which over this repository
  is 24 findings across the top-level docs and not one above `info`, and a hook
  that blocks a commit over a long line is a hook that gets deleted.
- **Every `diff` query takes the file-class predicate as a parameter** -
  `staged_files`, `changed_since`, `staged_hunks`, `hunks_since`,
  `hunks_between`. `check` passes `is_scan_target`, `lint-docs --staged` passes
  `is_markdown`. Generalizing the names query and leaving the hunks query
  hardcoded is the trap: the asymmetry is invisible until a command needs hunks
  for a different class, and then it silently gets the wrong one.
  Hardcoded, the markdown hook had no way to ask git what this commit touched
  and ran over the whole repository instead. `--staged` deliberately skips
  `files::expand_named`: that expander resolves an empty list to `root`, which
  is what makes bare `drep lint-docs` mean "this tree" and would turn "no
  markdown in this commit" into "lint every document", every commit.
- **Every `git diff` drep parses goes through `diff::git_diff`, every path
  it prints goes through `diff::quoting::decode`, and a hunk body is read by
  its counts.** `DIFF` pins the output format against the user's configuration
  and the repository's attributes: `diff.noprefix`, `diff.mnemonicPrefix`,
  `diff.dstPrefix`, `color.ui=always`, `diff.external`, a textconv driver,
  `diff.suppressBlankEmpty`, and a committed `binary` or `-diff` attribute or
  a NUL byte (`--text`) each once made the hunk parser find nothing, or text
  other than the committed text, and the gate reported the file clean.
  `--diff-filter=ACMRT` keeps `T`, because a symlink replaced by a regular
  file is a type change. git quotes a name holding `é`, `"`, `\` or a
  control character, and ends a `+++` name holding a space with a tab; read
  raw, either name matched no language and the file dropped out of `--staged`
  and `--diff` unreviewed. `parse_unified_diff` consumes exactly the lines the
  `@@` counts declare, as git's `apply.c` does: every body line is prefixed,
  so an added line reading `++ /dev/null` or `++ b/other.rs` stays content
  rather than closing the file or reattributing its later hunks.
  `src/diff/tests/output_format.rs` runs real git under each setting and name,
  and `src/diff/tests/review_evasion.rs` under each kind of content.
- **`LintOutcome` carries the gate's `Gating`, and the renderer reports it.**
  The footer needs to say whether the findings on screen blocked the run, and
  asking that question a second time in `render` is what `check` documents on
  `CheckOutcome::exit` and fixed. It is not derivable from `exit` either: a run
  with an unreadable file exits `Unanalyzed` whether or not its findings also
  crossed the threshold. `findings::any_at_or_above` is the one comparison
  behind both commands' gates.
- **The threshold governs findings, not failures.** `lint-docs` exits 2 for a
  file it could not read whatever `--fail-on` says, because that is the absence
  of analysis. It exits 1 for findings only at or above the threshold. Its
  startup path touches `docs` and `files` and nothing else: no config file, no
  provider chain, and no cache.
- **A link reference definition's URL is not a bare URL.** `[ref]: https://...`
  declares a target that is supposed to be bare; advice to wrap it in
  `[text](url)` would break the definition.
- **open-agent-sdk 0.11.3 or later is required**, and the floor has moved six
  times for reasons that are still live. 0.7.0: earlier versions silently
  discarded streamed text when a response ended without `finish_reason`, did
  not classify 429 as retryable, and could not leave `max_tokens` unset. 0.9.0:
  `ApiProtocol`, without which `kimi` and `minimax` are unreachable, and an
  omittable `temperature`, without which `k3` answers a 400 to every request.
  0.10.0: text arrives one `StreamEvent` per delta while the stream is open
  rather than as one block at its end, so a stream that reports no
  `finish_reason` on any chunk still delivers its text and finishes as
  `Unspecified` - the fixture
  `test_support::sse_without_finish_reason` needs that, and under 0.9.x it
  yielded nothing at all. Construct HTTP errors with
  `Error::api_status(code, msg)`; `Error::api(msg)` leaves `status: None` and
  is therefore never retryable. 0.11.0: caller-supplied request headers, plus a
  `u64` `RequestOptions.max_tokens` that matches the non-negative wire contract.
  0.11.2: both public request paths disable redirects, so a protocol key or
  caller credential cannot be replayed to a destination chosen by a response.
  0.11.3: request failures and history resets clear stale output, failed
  streams close cleanly, and unused runtime dependencies are removed without
  changing public signatures or defaults.
- **Every HTTP endpoint is an exact origin; drep never follows redirects.**
  Completion requests inherit `Policy::none()` from open-agent-sdk 0.11.2;
  `http::client` sets it independently for drep's model-listing and quirks
  fetchers. The split is load-bearing: `drep init` sends its own authenticated
  `GET /models` outside the SDK. reqwest removes `Authorization` on a
  cross-origin hop but not Anthropic's `x-api-key`, and a same-origin hop
  replays both, so selective stripping is not the contract. Raw loopback tests
  in `llm/models/tests/redirects.rs` prove that neither target is contacted and
  that the origin's `30x` status reaches normal classification.
- **`run_one_query` assembles the response; no single block is the answer.**
  The 0.10.0 break is invisible to the compiler, because the types did not
  change and only the number of events carrying the same text did. Reading
  `blocks[0]`, returning on the first `Text`, or joining fragments with a
  separator each yields something other than what the model said, and each
  parses as prose or as a different document rather than failing loudly.
  `src/llm/client/tests/streaming.rs` pins it by delivering identical bytes
  one way and then many ways and comparing the extracted JSON, with the splits
  falling inside a key and inside a number so a separator corrupts the parse
  instead of being forgiven as whitespace.
- **The release config is `dist-workspace.toml`, and
  `.github/workflows/release.yml` is generated from it.** Do not hand-edit the
  workflow. `cargo-dist-version` decides both
  which `dist` CI downloads and which one wrote the workflow, so editing the
  config without re-running `dist init` leaves the release planned by a version
  that never saw the change. `.github/build-setup.yml` is copied into the same
  generated workflow, so changing either source also requires `dist init`.
  The same test pins the target list, installers,
  tap and formula name because nothing else in the suite reads either file,
  and the first sign of a mistake is a release that already happened. GitHub is
  the only release authority.
- **`[profile.dist]` adds nothing to `[profile.release]`.** `dist init` writes
  it as `inherits = "release"` plus `lto = "thin"`, its own build-time default,
  which would revert the fat LTO, the single codegen unit and the `strip` this
  crate sets for the only binaries anyone installs. Re-running `dist init`
  leaves an existing `[profile.dist]` alone, so the pruned profile survives
  regeneration.
- **The Homebrew formula is named `drep`; only the crate is `drep-ai`.** The
  suffix exists because `drep` is taken on crates.io, and a tap is namespaced
  by its owner, so `brew install slb350/tap/drep` installs a binary
  of the same name. The shell installer keeps the crate name
  (`drep-ai-installer.sh`) because it is served from the GitHub release. dist
  gates the formula push on `!announcement_is_prerelease`, so a `-alpha` tag
  releases binaries and installer without the tap repository or
  `HOMEBREW_TAP_TOKEN` existing; the first stable tag needs both.
- **The arm64 Linux release cross-builds on x86_64 homelab-ai-1.** Its cargo-dist
  runner entry declares the x86_64 host explicitly, and the repository-owned
  build setup installs pinned Zig and cargo-zigbuild versions before the
  generated dependency step. Reqwest enables `native-tls-vendored` for that
  target only, because its SDK dependency enables native TLS and an arm64
  build cannot discover homelab-ai-1's x86_64 OpenSSL through pkg-config. Keep
  the TLS library self-contained rather than depending on mutable target sysroot
  packages on the runner, and do not make native targets compile it from
  source. The Apple and x86_64 Linux targets remain native builds.

The repository `.shellcheckrc` enables external source analysis so linting a
changed mutation script also checks its shared `mutants-common.sh` include.

## Releasing

Test maintenance and behavior-preserving refactors may remain under Unreleased.
A standalone release should provide a feature, user-facing fix, or material
runtime improvement.

**A released version's CHANGELOG section must stay small.** cargo-dist parses
the section matching the tag and embeds it twice in the plan manifest, and
`.github/workflows/release.yml` hands that manifest to the Homebrew publish job
as a single environment variable. Linux caps one environment variable at 128 KB
(`MAX_ARG_STRLEN`), so an oversized section makes `execve` fail with
"Argument list too long" *before* the job authenticates - which reads as a
broken `HOMEBREW_TAP_TOKEN` and is not one. Check with
`dist plan --output-format=json | wc -c` before tagging; there is no config key
that points dist at a different changelog file.

Everything before the formula push survives such a failure - the binaries, the
installer and the GitHub release are already published - so the recovery is to
push `drep.rb` from the release assets to the tap by hand and fix the section.

The version in `Cargo.toml` is the single source. The annotated tag `vX.Y.Z` on main drives both publication paths: `.github/workflows/release.yml` builds the four targets, creates the GitHub release and pushes the Homebrew formula, and `.github/workflows/publish-crate.yml` publishes the same tagged commit to crates.io through trusted publishing, since cargo-dist does not publish Rust crates. `publish-crate.yml` runs in the `release` environment, which deploys only from main and `v*` tags, and the crate's crates.io trusted publisher requires it. Run `cargo publish --dry-run --locked` before tagging. Jobsy's release flow (Scheduled maintenance, below) does both; by hand:

```sh
cargo publish --dry-run --locked
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

A stable tag needs both prerequisites in place: the `slb350/homebrew-tap`
repository, and a `HOMEBREW_TAP_TOKEN` secret with `repo` scope on
`slb350/drep`. A prerelease tag (`v2.0.0-alpha.1`) does not - dist gates the
formula push on `!announcement_is_prerelease`, so that job skips.

## Scheduled maintenance

Jobsy on homelab-ai-1 runs this repository's scheduled maintenance as a weekly dependency-security job, a weekly improvement job, and a weekly security review. Each works in a fresh checkout of main and opens a draft pull request from a `jobsy/` branch.

A daily Jobsy review job reviews each open `jobsy/` pull request against this file, fixes it on its own branch when it is not solid, and merges it with a merge commit once every check on the reviewed head has passed; Jobsy refuses the merge otherwise. A weekly Jobsy release job decides under the release rules in this file whether the merged work warrants a release, and if so opens a `jobsy/release/` pull request that bumps the version and moves the changelog entries. When that pull request merges, Jobsy pushes the annotated tag on the merge commit, and the release and publish-crate workflows publish it. No Jobsy job pushes main directly or publishes from its own host.

Mutation testing runs in CI on each Jobsy pull request: `rust.yml` mutates the pull request's diff once Linux and macOS validation pass, and again the diff pushed to main after a merge; `mutants.yml` runs the weekly full sweep.

## Remotes

- `origin` (GitHub, public and release authority):
  `git@github.com:slb350/drep.git`, using the configured GitHub SSH key
