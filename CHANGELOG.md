# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added - 2026-08-16
- **Local pre-commit gate** (`.pre-commit-config.yaml`): ruff check, ruff format,
  `drep lint-docs`, and `drep check --staged --fail-on error`, all run from `./venv` via
  `language: system`. Installed into `.git/hooks/pre-commit`, which the machine's global
  `core.hooksPath` chainer execs. `lint-docs` runs report-only here on purpose - its
  `long_line` check contradicts this repo's `MD013: false`.
- **`drep check --fail-on {info,warning,error}`** (default `info`, unchanged behaviour):
  the severity at or above which a finding blocks. Everything is still reported; only the
  exit code changes. Without it a commit gate is unusable - the LLM emits info-level style
  suggestions on almost every file, so exit 1 on any finding means no commit ever passes.
- **`drep lint-docs --strict`**: exits 1 when issues are found, so the rule-based markdown
  checks can gate a commit. Default behaviour is unchanged (report, exit 0).
- **`drep lint-docs` accepts multiple paths** (`nargs=-1`, defaults to `.`), which is what
  pre-commit passes.
- **`drep-lint-docs` hook id** in `.pre-commit-hooks.yaml` for downstream consumers.

### Changed - 2026-08-16
- **Scanner passes return `AnalysisResult(findings, failed_files)`** instead of a bare
  finding list. `analyze_code_quality` / `analyze_docstrings` now name the files they could
  not analyze, rather than leaving the caller to infer it from a progress callback.
  `drep scan` warns when a scan was incomplete instead of printing only "Found N issues".
- **Shared pruning walk** — `file_targets.walk_targets()` is the one directory traversal;
  `RepositoryScanner.get_scan_targets` and `lint-docs` both use it.
- **`CodeQualityAnalyzer.analyze_files` deprecated** (no production callers; removal in
  1.3.0), matching the existing `ParallelAnalyzer` deprecation.

### Fixed - 2026-08-16
- **`drep check` reported unanalyzed files as clean.** `CodeQualityAnalyzer.analyze_file`
  and `DocstringGenerator._generate_docstring` swallowed every exception and returned
  empty, so an unreachable LLM endpoint printed "✓ No issues found" and exited 0 — a
  pre-commit hook that rubber-stamped every commit. Both now propagate; `RepositoryScanner`
  counts the file as failed; `_run_check` returns `CheckOutcome(findings, unanalyzed)`; and
  `drep check` exits **2** with "N file(s) could not be analyzed".
- **`max_retries: 0` never sent the request.** `range(self.max_retries)` skipped the loop
  entirely and raised `RuntimeError("LLM request failed but no exception was captured")`,
  hiding the real transport error. Attempts now floor at 1, so failures name their cause.
- **`lint-docs` walked ignored directories**, reporting on `venv/`, `build/`, and
  `*.egg-info`. It now uses the shared pruning walk (126ms → 1.6ms here, and 38 real
  files instead of 62).
- **Markdown check false positives** — 1668 findings across this repo became 256:
  - `missing_space_after_heading` fired on *every* well-formed heading below level 1:
    `^#{1,6}\S` backtracks to one `#` and matches the second one.
  - Heading checks ignored code fences, flagging `#!/bin/bash` in every bash sample.
  - `link_syntax_invalid` counted parentheses line-locally, flagging any prose
    parenthetical that wrapped onto a second line.
  - `bare_url` flagged URLs inside inline-code spans.
  - `bare_url` *missed* a real bare URL whenever any well-formed link appeared on the same
    line. Well-formed links are now blanked and the remainder searched, rather than the
    whole line being excused (this is why the repo baseline is 269, not 256).
  - Dead `else 1` branch in the `empty_heading` level calculation - `_HEADING_EMPTY` only
    matches a run of `#`, so `split()` is never empty.
- **`drep check --format json` emitted a status line on stdout**, so "JSON output for tools"
  did not parse. `Checking N file(s)...` now goes to stderr.
- **A `content: null` response crashed four frames deep** as "argument of type 'NoneType'
  is not a container or iterable", from inside the JSON parser. Reasoning models that spend
  their whole budget on `reasoning` return exactly that. The client now raises a named
  error quoting `finish_reason`, so `length` points straight at `max_tokens`.
- **The poisoned entry was cached.** The bad response was written to the cache *before* the
  parser choked, so every later run replayed `content=None` in 0.6s without ever calling
  the LLM again. A cached entry with empty content is now treated as a miss.
- **`is_ignored_dir` was case-sensitive** while the module promised "identical,
  case-insensitive decisions" - a directory named `VENV` or `.Git` was descended into and
  analyzed. It now casefolds, matching the suffix predicates.
- **A response without a `usage` block crashed the SDK path.** `usage` is optional in the
  OpenAI schema and some local servers omit it; the HTTP fallback already defaulted it,
  the open-agent-sdk path did not. Since analyzer failures now propagate, that latent
  `AttributeError` would have marked the file unanalyzed rather than being swallowed.

### Planned
- Vector database integration for cross-file context
- Custom rule definitions
- Integration with existing linters (pylint, eslint, etc.)
- Metrics dashboard
- Notification system (Slack, Discord)
- Multi-repository analysis features
- Removal of deprecated `ParallelAnalyzer` / `timeout_with_partial_results` (1.3.0)

## [1.2.0] - 2026-08-16

Audit-driven correctness and simplification release. All 24 accepted findings from the
simplification audit resolved; rejected findings (C9, C11) documented with rationale.
A second `/simplify` pass over the merged branch (reuse, simplification, efficiency,
altitude) resolved a further round of findings — recorded below under their headings.

### Fixed - Reliability
- **Rate-limit permit leak on cancelled entry** (C3): cancellation or a rate-check error
  during `RateLimitContext.__aenter__` leaked the already-acquired global/repo semaphore
  permits permanently (a leaked repo permit blocked that repo forever). Permits are now
  rolled back on any `BaseException` mid-entry.
- **Unobserved background-task failures** (C19): failed webhook scans surfaced only as
  asyncio warnings; the done-callback now retrieves and logs exceptions with context.
- **Env substitution re-typed config values** (C16): the yaml.dump → regex → reload
  round-trip re-parsed env values as YAML (`true` became bool, `123` became int,
  `a: b` corrupted structure). Substitution now runs over the parsed tree, strings only.
- **Aggregate metrics lost latency and analyzer data** (C6): `to_dict` persists raw
  `total_latency_ms`; a model-owned `merge_serialized` owns the full field set (latency
  total, `by_analyzer`) with legacy backfill. `drep metrics --days` reports real averages.
- **Nested-function detection missed control-flow nesting** (C12): helpers defined under
  `if`/`try`/`with` inside a function leaked in as docstring candidates.
- **Webhook payload shape** (C18): non-object JSON bodies now get a 400 instead of a 500.

### Fixed - Correctness of the review pipeline
- **Review results bound to their diff** (C13): `review_pr` returns an immutable
  `PreparedReview` (result, anchor, per-file added lines); `post_review` consumes it.
  Previously a second review silently re-anchored the first review's line validation.
- **One anchor fetch per review** (C2): new `BaseAdapter.get_review_anchor()`; GitLab
  validates and carries MR `diff_refs` (base/head/start SHAs) once per review instead of
  refetching the MR for every inline comment (n comments = n MR fetches, with mid-loop
  version drift).
- **GitHub adapter duplication** (C1): the SHA-explicit inline-comment method is now
  canonical and owns the 422 invalid-line handling; the one-shot variant resolves the
  anchor and delegates.

### Changed
- **File-size discipline**: every Python file is now under the 800-line limit.
  `drep/llm/client.py` split into `rate_limiter`/`json_parsing`/`git_utils` (public names
  re-exported), `drep/cli.py` into `cli_wizard`/`cli_workflows`, adapter review/PR methods
  into mixin modules (`gitlab_prs`, `gitlab_reviews`, `github_reviews`), and the largest
  test files split by topic. No public names removed.
- **File-target policy unified** (C7): one case-insensitive suffix policy across full
  scans, commit diffs, staged files, and per-analyzer filters. `TEST.PY` is no longer
  silently never analyzed.
- **Circuit breaker is real** (C5+C23): HALF_OPEN is an exclusive single-probe state
  (dead `half_open_max_calls` removed); both provider transports run through the
  breaker, which previously was constructed but never invoked. Open circuits fail fast
  instead of being retried.
- **Version single-sourced** (C24): `drep.__version__` is the only version declaration
  (pyproject reads it dynamically; FastAPI app imports it). Was drifting across four
  places.
- **Platform selection single-sourced** (C17): duplicated gitea→github→gitlab chains in
  the scan and review workflows collapsed into `_resolve_platform`.
- **CI selects tests by marker** (C21): new `external_service` marker on the truly-live
  test modules; CI runs `-m "not external_service"` instead of name-based exclusion,
  restoring ~43 silently-excluded hermetic tests to CI (839 vs 796).
- **`issue_number` is NOT NULL** (C20): a NULL row suppressed a finding forever; legacy
  SQLite tables are migrated (NULL rows dropped so findings can be re-reported).

### Fixed - Concurrency and rate limiting (second pass)
- **Rate limiter held its lock across sleeps**: both `_check_request_rate_limit` and
  `_check_token_rate_limit` slept while holding the shared lock, which blocked
  `RateLimitContext.__aexit__` — needing the same lock to reconcile tokens before
  releasing permits — and stalled all LLM traffic for the duration of the wait.
- **Unsatisfiable token reservation spun forever**: a request estimating more tokens
  than the entire per-minute budget could never satisfy the bucket condition and looped
  indefinitely (previously while holding the lock, deadlocking every request).
  `RateLimiter.request()` now clamps the reservation.
- **Concurrency permits taken before rate waits**: a merely time-throttled request
  occupied one of the scarce `max_concurrent` slots while sleeping, dropping effective
  concurrency well below the configured value. The requests-per-minute wait now happens
  first.
- **Circuit-breaker probe reservation could be cleared by another task**: `finally`
  cleared `_probe_in_flight` unconditionally, so a CLOSED-state call finishing after the
  breaker re-entered HALF_OPEN wiped a probe reservation and let probes dogpile the
  recovering service. Only the reserving call clears it.
- **Webhook config failures no longer fail open**: `_load_webhook_secret` caught every
  exception and returned `None`, downgrading the endpoint to accepting unauthenticated
  requests on a YAML typo. A missing config still means "unset"; an unreadable one is
  now rejected with 503.
- **`ValidationError` escaped the JSON fallback ladder**: a response that parsed but
  failed schema validation propagated out of `extract_json`, past the stricter-prompt
  retry in `analyze_code_json` that exists for exactly that case. All strategies now
  share one recoverable-exception policy (and no longer swallow real bugs via bare
  `except Exception`).
- **Gitea inline-comment fallback retried on any status**: the `new_position` →
  `position` retry exists for older Gitea versions rejecting the field name (400/422),
  but fired on 401/403/404/5xx too, issuing a pointless second POST and burying the real
  cause in a compound error message.

### Changed - Performance (second pass)
- **Per-file LLM analysis runs concurrently**: `analyze_code_quality` and
  `analyze_docstrings` awaited one round trip at a time, so a scan took the *sum* of all
  LLM latencies while the configured `max_concurrent_global`/`max_concurrent_per_repo`
  went unused. Both now gather; the rate limiter supplies back-pressure.
- **Review fetches the PR once**: `get_review_anchor` internally called `get_pr`, then
  the prompt fetched the identical payload again. Anchor derivation is now the pure
  `anchor_from_pr(pr_data, ...)` hook, and the PR and diff are fetched concurrently.
  The diff is also no longer re-serialized from parsed hunks for the prompt.
- **Per-repo rate limiting reached PR reviews**: `repo_id` was computed and threaded
  through `_analyze_diff_with_llm` but never passed to the LLM client, silently
  disabling per-repo limiting for reviews. It and `commit_sha` (cache key) now are.
- **Scan discovery prunes ignored trees**: `rglob("*")` stat'd every entry under `.git`,
  `venv`, `build` before filtering; discovery now walks with in-place directory pruning.
- **Cheaper hot paths**: idle repo-semaphore eviction sweeps once per TTL instead of
  scanning all tracked repos per request; `request_times` is a deque; the webhook secret
  is cached per (path, mtime) instead of re-parsing the config per request; the LLM cache
  tracks a running byte total instead of stat'ing the whole cache directory on every
  write; the metrics history file is capped and written off the event loop;
  `git rev-parse` runs via `asyncio.to_thread`; `fuzzy_inference` regexes are compiled
  once per (schema, field); the issue-manager dedup check is one query instead of one
  per finding; `_collect_function_nodes` no longer walks function bodies to build lists
  it always discarded.

### Changed - Structure (second pass)
- **Adapter contract shrank to what actually varies**: `post_review_comment` and
  `close()` are concrete on `BaseAdapter` (three byte-identical overrides removed;
  Gitea gains close()'s error handling). A shared `_network_error()` replaces 15 copies
  of the timeout/connect cascade, and `_decode_file_content()` the duplicated base64
  decode. `git_clone_url()` is a new adapter method, so GitHub Enterprise and GitLab
  self-hosted URL rules leave the shared workflow.
- **Mixins inherit instead of restating their host**: `GitHubReviewMixin`,
  `GitLabPrMixin` and `GitLabReviewMixin` derive from `BaseAdapter`/`GitLabMixinBase`
  rather than each re-declaring the host surface in a duplicate `if TYPE_CHECKING` block.
- **Platform identity derives from the typed data**: each `*PlatformData` variant carries
  `config_key`/`display_name`/`token_env_var`, removing four scattered mappings —
  including a `"${LLM_API_KEY}" in yaml.dump(config_dict)` string search in the CLI for a
  fact the model already held, and an isinstance chain whose fallthrough silently
  labelled unknown variants "gitlab".
- **New shared modules**: `drep/core/file_targets.py` (one file-target policy the
  analyzers reuse instead of re-implementing suffix checks), `drep/logging_utils.py`
  (one secret-redaction helper, previously inlined in three places),
  `drep/llm/http_compat.py` (the OpenAI-shaped HTTP shim, previously six classes rebuilt
  inside every `LLMClient.__init__` and closing over the whole init frame).
- **`load_raw_config()`** in `drep/config.py` is shared by `load_config` and the webhook
  server, which had hand-rolled the same read/substitute sequence and reached across
  modules for a private helper.
- **`_run_check` moved to `drep/cli_workflows.py`** beside `_run_scan`/`_run_review`, and
  returns findings instead of printing them.
- **The `finding_cache` migration builds its table from the model** rather than a raw DDL
  string that could silently drop a future column, and probes on a read-only connection.
- **`RepositoryScanner._get_all_python_files` → `get_scan_targets`** (it returns `.md`
  too), and `PRReviewAnalyzer`/`RepositoryScanner` take `adapter`, not `gitea_adapter`.

### Breaking Changes (second pass)
- **`PlatformConfig.env_var` is a derived property**, no longer a constructor argument.
- **`BaseAdapter.git_clone_url(owner, repo)` is abstract** — custom adapters must
  implement it. `post_review_comment` and `close()` are no longer abstract.
- **`PRReviewAnalyzer(llm_client, adapter)`** — the second parameter was `gitea_adapter`.
- **`RepositoryScanner(db, config, adapter=...)`** — the keyword was `gitea_adapter`.

### Security
- **Webhook HMAC verification** (out-of-audit finding): optional `webhook_secret`
  config field enables constant-time HMAC-SHA256 verification of `X-Gitea-Signature`
  over the raw body; requests with missing/invalid signatures are rejected with 403.
  Unset secret keeps existing deployments working (warning logged).

### Removed
- **`drep/security/` package** (C10): `detect_secrets_in_logs`/`sanitize_url` had zero
  production callers; safe-logging practices live inline in the adapters.
- **Six TODO-stub modules** (S15): `core/llm_agent`, `core/code_analyzer`,
  `db/repository`, `models/webhook`, `documentation/llm_review`,
  `documentation/comment_gen`.
- **Personal endpoint hardcoded in `scripts/test_llm_client.py`** (C25): env-driven
  (`DREP_TEST_LLM_ENDPOINT`) with a neutral localhost default.

### Deprecated
- `ParallelAnalyzer` and `timeout_with_partial_results` (C8): zero production callers;
  emit `DeprecationWarning`, removal planned for 1.3.0.

### Breaking Changes
- **`llm.provider` is a closed set: `openai-compatible` | `bedrock`** (C14). The wizard
  no longer offers `anthropic` (it generated configs guaranteed to fail at runtime).
  Migration: use `provider: openai-compatible` with an OpenAI-compatible Anthropic
  proxy endpoint, or `provider: bedrock` with Claude model IDs.
- **Wizard wrappers are typed-only** (C15): `PlatformConfig`/`LLMConfig`/
  `DocumentationConfig` no longer accept the deprecated raw-dict `config=` field;
  platform key, display name, and provider derive from the typed data.
- **`create_pr_review_comment(anchor, file_path, line, body)`** (C2/C13): takes an
  immutable `ReviewAnchor` from `get_review_anchor()` instead of
  `(owner, repo, pr_number, commit_sha)`. GitLab requires a `GitLabReviewAnchor`.
- **`PRReviewAnalyzer.post_review(prepared)`** (C13): takes the `PreparedReview`
  returned by `review_pr`.
- **`FindingCache.issue_number` is NOT NULL** (C20).

## [1.1.3] - 2026-08-16

### Changed - Tooling & Code Modernization 🧹

**Maintenance:** Internal quality improvements only — no user-facing behavior changes.

- **Ruff expanded from 3 rule groups to 16** (added pyupgrade, bugbear, simplify,
  comprehensions, return, pie, perflint, use-pathlib, refurb, ruf, pylint C/E) and
  the entire codebase brought into compliance (~450 issues resolved, including all
  101 `raise`-without-`from` exception-chaining violations).
- **Black replaced by `ruff format`** — one tool for lint + format; `black` removed
  from dev dependencies.
- **CI modernized**: lint/format/type checks split into a fast `lint` job; test
  matrix now covers Python 3.10–3.14 (previously 3.13 only); actions bumped
  (`checkout@v5`, `setup-python@v6`).
- **Refactors**: deduplicated function/method AST extraction in
  `drep/docstring/ast_utils.py`; hoisted stray function-level imports (documented
  lazy imports kept with `noqa`); `open()` calls migrated to `pathlib`.
- **Dependencies refreshed** across the board (`uv.lock` regenerated).

### Fixed
- **Fire-and-forget webhook tasks could be garbage-collected mid-run**:
  `drep/server.py` now holds strong references to background scan/review tasks
  until they complete (RUF006).

## [1.1.2] - 2026-05-24

### Changed - Type-Safety Hardening 🔍

**Maintenance Release:** Internal type-safety improvements only — no user-facing
behavior changes.

- **mypy is now clean across `drep/`** (resolved all 62 pre-existing errors across
  11 modules) and runs as a CI gate, so type regressions are caught automatically.
- **`BaseAdapter` contract completed**: added `create_pr_review_comment`, which all
  three adapters already implemented and the PR-review workflow depends on. The
  `PRReviewAnalyzer`/`RepositoryScanner` adapter parameters were widened from
  `GiteaAdapter` to `BaseAdapter`, so `drep review` against GitHub/GitLab is now
  correctly typed.
- **Modernized SQLAlchemy models** to use the 2.0 `DeclarativeBase`.

### Fixed
- **Incorrect exception reference** in the GitHub and GitLab adapters: caught
  `base64.binascii.Error` (which only resolved by accident) is now the correct
  `binascii.Error`.

### Development
- Added `[tool.mypy]` configuration (pydantic plugin; `ignore_missing_imports` for
  boto3/botocore) and a `mypy drep` step to the CI workflow.

## [1.1.1] - 2026-05-24

### Fixed

- **Gitea inline review comments rejected with "review event requires a body"** (#11):
  `GiteaAdapter.create_pr_review_comment()` submitted the pull review with an
  empty top-level `body`. Some Gitea versions enforce a non-empty body even when
  inline comments are present, so every inline comment request was rejected and
  no review feedback was posted.
  - Both the `new_position` and `position` payloads now send a short generic
    placeholder (`REVIEW_BODY_PLACEHOLDER`) as the review body.
  - The actual finding still appears only in the inline comment, so it is not
    duplicated as a review summary (preserving the original empty-body intent).

### Testing
- **1 new test added** (796 total passing, excluding integration)
- `test_create_pr_review_comment_always_sends_non_empty_body`: regression test
  asserting a non-empty body is sent even when the inline comment text is empty
- Updated `test_create_pr_review_comment_sends_correct_payload` to assert the
  placeholder body

### Development
- Fix contributed via PR #12 by Rain Wu (@facewhy); refined to use an
  unconditional placeholder rather than echoing the finding text.

## [1.1.0] - 2025-11-09

### Added - Interactive Configuration Wizard 🧙‍♂️

**Feature Release:** Comprehensive `drep init` wizard for guided configuration setup!

- **Interactive CLI Wizard**: Step-by-step configuration generator
  - Platform selection (Gitea, GitHub, GitLab)
  - Enterprise server detection (GitHub Enterprise, self-hosted GitLab/Gitea)
  - Repository pattern configuration with wildcard support
  - LLM provider selection (OpenAI-compatible, AWS Bedrock, Anthropic)
  - Documentation analysis settings with markdown linting options
  - Custom database URL configuration
  - Advanced LLM settings (temperature, max tokens, rate limits)
  - Environment variable verification and guidance

- **Config Discovery**: Flexible configuration file locations
  - Current directory: `./config.yaml` (project-specific)
  - User config directory: `~/Library/Application Support/drep/config.yaml` (system-wide)
  - User selects preferred location during wizard

- **Input Validation**: Real-time validation with helpful error messages
  - `URLType`: HTTP/HTTPS URL validation with scheme checking
  - `RepositoryListType`: Repository pattern validation (`owner/repo`, `owner/*`)
  - `BedrockModelType`: AWS Bedrock model ID validation (anthropic.*, amazon.*, etc.)
  - `DatabaseURLType`: SQLAlchemy database URL validation
  - `NonEmptyString`: Required field validation
  - Duplicate repository pattern detection and deduplication

- **Strongly-Typed Wizard Models**: 7 new frozen dataclasses
  - `GitHubPlatformData`: GitHub platform configuration with optional enterprise URL
  - `GiteaPlatformData`: Gitea platform configuration
  - `GitLabPlatformData`: GitLab platform configuration
  - `OpenAILLMData`: OpenAI-compatible LLM configuration
  - `BedrockLLMData`: AWS Bedrock LLM configuration with region/model
  - `BedrockRegionModel`: Nested Bedrock region and model settings
  - `AnthropicLLMData`: Anthropic API configuration
  - `DocumentationConfigData`: Documentation analysis settings
  - All models use tuples for immutability (not lists)
  - `to_dict()` methods convert tuples → lists for YAML serialization

### Testing - Security & Integration
- **13 new tests added** (795 total tests passing)

**Finally Block Error Handling (3 tests)**:
- Test cleanup errors don't mask scan errors (ValueError vs OSError)
- Test successful cleanup allows scan error propagation
- Test cleanup failure is silent when main operation succeeds
- **Finding**: All finally blocks already correct - verification tests added

**Token Leakage Prevention (5 tests)**:
- Test GitHub token never logged (caplog + stdout + config file)
- Test Gitea token never logged
- Test Anthropic API key never logged
- Test environment variable checks mask token values
- Test multiple tokens all masked simultaneously
- **Security**: Wizard uses placeholders (`${GITHUB_TOKEN}`) not actual values

**End-to-End Integration (5 tests)**:
- Test GitHub end-to-end (wizard → load_config → adapter creation)
- Test Gitea + Bedrock end-to-end (complex nested config)
- Test GitLab + Anthropic end-to-end
- Test custom database URL configuration
- Test malformed YAML caught gracefully by validator
- **Integration**: Verifies complete workflow from wizard → scan

### Changed
- **CLI Workflow**: `drep init` now generates fully validated configurations
- **User Experience**: Guided setup replaces manual YAML editing
- **Error Prevention**: Input validation catches issues before config file creation
- **Security**: Environment variable placeholders prevent token leakage

### Improved
- **Validation**: Click validators provide immediate feedback during input
- **Documentation**: Inline help text and examples throughout wizard
- **Defaults**: Sensible defaults for all optional settings (temperature=0.2, max_tokens=4000)
- **Error Messages**: Clear, actionable messages for validation failures

### Development
- **Zero Tech Debt Policy**: All 3 critical PR review issues resolved
- **TDD Methodology**: All 13 tests written first, then features implemented
- **Type Safety**: Strongly-typed wizard models prevent runtime errors
- **Immutability**: Frozen dataclasses with tuple-based collections

## [1.0.0] - 2025-11-09

### Added - GitLab Platform Support (Phase 3.5) 🎉

**Production Release:** drep now supports all three major git platforms!

- **GitLab Adapter**: Complete GitLab REST API v4 implementation
  - Full BaseAdapter compliance (all 8 abstract methods)
  - Support for both GitLab.com and self-hosted instances
  - URL-encoded project paths (owner%2Frepo)
  - PRIVATE-TOKEN authentication header
  - Merge request (MR) reviews with discussion API
  - Position objects for inline comments
  - Base64-encoded file content support
  - Diff reconstruction from JSON array format

- **Platform Coverage**: Production-ready support for:
  - ✅ Gitea (self-hosted and Gitea.com)
  - ✅ GitHub (GitHub.com and GitHub Enterprise)
  - ✅ GitLab (GitLab.com and self-hosted instances)

- **API Compatibility Fixes**:
  - Normalized `get_pr()` response to include `head.sha` field
  - Added `create_pr_review_comment()` method for PRReviewAnalyzer compatibility
  - Consistent API across all three platform adapters

### Testing
- **93 GitLab adapter tests** (up from 35 after fixes)
  - Comprehensive JSON validation tests
  - Network error handling (timeout, connection failures)
  - HTTP error code tests (401, 403, 500, 503)
  - Rate limit edge cases with parametrized tests
  - URL handling tests (/api/v4 suffix deduplication)
- **618 total tests passing** - All platforms verified
- **Test coverage**: 0.082 test/line ratio (71% above GitHub adapter)

### Changed
- **Production Status**: Development Status classifier updated to "5 - Production/Stable"
- **Platform Parity**: All three adapters (Gitea, GitHub, GitLab) feature-complete
- **CLI Integration**: `drep scan` and `drep review` commands support all platforms
- **Documentation**: Updated all docs to reflect GitLab support

### Improved
- **Error Handling**: GitLab adapter has superior error handling vs existing adapters
  - Consistent JSON validation across all endpoints
  - Comprehensive network error detection
  - Clear, actionable error messages with context
  - Proper rate limit detection and reporting

### Fixed
- **Codex Bot Issues** (PR #8):
  - Fixed missing `head.sha` field in GitLab MR responses
  - Fixed missing `create_pr_review_comment()` method
  - Both issues resolved for CLI compatibility

### Development
- **Zero Tech Debt Policy**: All critical issues resolved before release
- **Comprehensive Reviews**: Multi-agent code review process
- **TDD Methodology**: All features developed test-first
- **157% Test Increase**: From 35 to 93 tests during development

## [0.9.0] - 2025-11-08

### Added - Pre-Commit Hook Support (Phase 3.6)
- **New `drep check` command**: Local-only analysis without platform API requirements
  - `--staged` flag: Check only git staged files (pre-commit workflow)
  - `--exit-zero` flag: Warning mode without blocking commits
  - `--format` option: Output as `text` (default) or `json`
  - Works without Gitea/GitHub/GitLab tokens (local-only mode)
  - Respects LLM config when present for intelligent analysis
  - Pre-commit friendly output format (`file:line:column: severity: message`)

- **Pre-commit Integration**: `.pre-commit-hooks.yaml` in repository
  - `drep-check` hook: Checks staged files only
  - `drep-check-all` hook: Checks all Python files
  - Direct repo reference: `repo: https://github.com/slb350/drep`
  - Installation: `brew tap slb350/drep && brew install drep-ai` or `pip install drep-ai`

- **Staged File Detection**: `RepositoryScanner.get_staged_files()` method
  - Returns only Python (.py) and Markdown (.md) files
  - Handles new files, deleted files, and renamed files correctly
  - Designed specifically for pre-commit workflow

### Changed
- **Config Validation**: Platform config now optional for local-only mode
  - `load_config()` accepts `require_platform=False` parameter
  - Enables LLM-only configurations without Gitea/GitHub/GitLab
  - `Config.require_platform_config` field controls validation
  - Backward compatible (default behavior unchanged)

- **Exit Codes**: `drep check` returns exit code 1 when issues found
  - Properly blocks commits in pre-commit hooks
  - Use `--exit-zero` for warning-only mode

### Testing
- **12 New Tests**: Comprehensive TDD coverage
  - 6 tests for `get_staged_files()` method
  - 4 tests for optional platform config
  - 4 tests for `drep check` command
  - All 521+ tests passing

### Documentation
- `.pre-commit-hooks.yaml`: Pre-commit hook definitions
- Pre-commit integration ready (detailed docs in README to follow)

### Development Methodology
- **Strict TDD**: All features developed with Test-Driven Development
  - RED: Write failing tests first
  - GREEN: Implement to pass tests
  - REFACTOR: Improve code quality
  - COMMIT: Commit each TDD cycle

## [0.8.2] - 2025-11-08

### Added
- **Interactive Platform Selection**: `drep init` now prompts for platform choice
  - Interactive prompt with GitHub, Gitea, GitLab options
  - Default to GitHub (most common use case)
  - Platform-specific config templates generated automatically
  - Correct environment variable names per platform (GITHUB_TOKEN, GITEA_TOKEN, GITLAB_TOKEN)

### Improved
- **README Documentation**: Comprehensive setup guide with step-by-step instructions
  - Clear platform selection guidance
  - Detailed API token creation instructions for each platform
  - LLM backend setup options (LM Studio, Ollama, AWS Bedrock)
  - Reduced user confusion during initial setup
- **User Guidance**: Better error messages and next steps after `drep init`

### Changed
- `drep init` command behavior: Now interactive instead of generating Gitea-only config
- Default platform: GitHub (changed from Gitea)

### Fixed
- User confusion when trying to scan GitHub repositories with default Gitea config
- Missing platform-specific setup instructions

## [0.8.0] - 2025-11-08

### Added - AWS Bedrock Provider Support (Phase 3.3)
- **AWS Bedrock LLM Provider**: Full support for Claude models via AWS Bedrock
  - BedrockClient implementation with OpenAI-compatible interface
  - Support for Claude Sonnet 4.5 and Haiku 4.5 models
  - Automatic AWS credential chain authentication
  - Region-specific model deployment
  - Comprehensive error handling for AWS-specific errors
- **Configuration Enhancements**:
  - `BedrockConfig` for AWS region and model selection
  - Optional `endpoint` and `model` fields for Bedrock provider
  - Provider-specific validation (`openai-compatible` vs `bedrock`)
  - Support for `provider="bedrock"` in LLMConfig
- **Test Coverage**: 511 total tests (19 new Bedrock-specific tests)
  - Unit tests for BedrockClient (17 tests)
  - Integration tests for LLMClient with Bedrock (4 tests)
  - Configuration validation tests (3 tests)

### Fixed - Critical P1 Issues
- **Cache Corruption Fix** (P1): Preserve Bedrock model name in `LLMClient.model`
  - Previously: Different Bedrock models shared cache entries (model=None)
  - Impact: Model A could serve stale responses from Model B
  - Fix: Explicitly set `self.model = bedrock_model` during initialization
  - Result: Each model has distinct cache keys, metrics show actual model names
- **Async Event Loop Blocking** (P1): Wrap boto3 calls in `asyncio.to_thread()`
  - Previously: Synchronous `boto3.invoke_model()` blocked event loop
  - Impact: Defeated async concurrency, stalled rate limiting/progress tracking
  - Fix: Use `asyncio.to_thread()` to run boto3 in thread pool
  - Result: Event loop remains responsive, concurrent requests work properly
- **AWS API Compliance** (P1): Add required headers and encode body as bytes
  - Previously: Missing `contentType` and `accept` headers, body as string
  - Impact: Violates AWS Bedrock API spec, could cause ValidationError
  - Fix: Add `contentType="application/json"`, `accept="application/json"`, encode body as bytes
  - Result: Full AWS API compliance per boto3 documentation
- **Config Validation** (P1): Make `endpoint` and `model` optional for Bedrock
  - Previously: Required dummy values for Bedrock configs
  - Impact: Made feature unusable as documented
  - Fix: Optional fields with provider-specific validation
  - Result: Bedrock works without dummy endpoint/model values
- **Endpoint Handling** (P1): Handle `endpoint=None` gracefully
  - Previously: `endpoint.rstrip("/")` crashed with AttributeError on None
  - Impact: Blocked Bedrock initialization
  - Fix: Check if endpoint exists before calling methods
  - Result: Bedrock provider initializes with endpoint=None

### Fixed - Non-Blocking Issues
- **StreamingBody Resource Management**: Added explicit `close()` calls
  - Ensures proper cleanup of AWS response streams
  - Prevents resource leaks in long-running processes
- **Error Message Clarity**: Enhanced user-friendly AWS error messages
  - ThrottlingException, AccessDeniedException, ValidationException
  - Actionable guidance for common Bedrock errors
- **Code Quality**: Addressed all PR review feedback
  - Removed redundant exception handlers
  - Added explanatory comments for complex logic
  - Improved test coverage for edge cases

### Changed
- **Documentation Updates**:
  - README: Added AWS Bedrock setup instructions and configuration examples
  - Technical Design: Updated with Bedrock architecture details
  - LLM Setup Guide: Comprehensive Bedrock configuration walkthrough
  - Roadmap: Marked Phase 3.3 complete, added Phase 3.4 (Anthropic Direct)
- **Dependencies**:
  - Added `boto3` for AWS Bedrock support
  - Added `botocore` for AWS SDK functionality

### Development
- **TDD Methodology**: All fixes implemented with strict Test-Driven Development
  - RED phase: Write failing tests first
  - GREEN phase: Implement fixes
  - REFACTOR phase: Improve code quality
  - VERIFY phase: Run full test suite
- **Code Quality**: All ruff/black checks passing
- **Zero Technical Debt**: All P1 and non-blocking issues resolved

## [0.1.0] - 2025-10-19

### Added
- Initial release of drep (PyPI package: drep-ai)
- Platform adapters for Gitea, GitHub, and GitLab
- Three-tiered documentation analysis:
  - Layer 1: Dictionary spellcheck
  - Layer 2: Pattern matching for common issues
  - Layer 3: LLM-based analysis for complex cases
- Code analyzer with AST parsing and LLM-based detection
- Documentation specialist features:
  - Typo detection and correction
  - Grammar and syntax checking
  - Missing comment detection and generation
  - Bad comment identification and improvement
- Automated draft PR creation for documentation fixes
- Issue creation for code quality problems
- FastAPI webhook server for receiving platform events
- Background worker for asynchronous job processing
- SQLite database for finding cache and deduplication
- Click-based CLI with commands:
  - `drep init` - Initialize configuration
  - `drep serve` - Start webhook server
  - `drep scan` - Manual repository scan
  - `drep validate` - Validate configuration
- Configuration via YAML file with environment variable support
- Docker support with docker-compose example
- Support for multiple LLM backends via open-agent-sdk:
  - Ollama
  - llama.cpp
  - LM Studio (OpenAI-compatible)
- Support for multiple programming languages:
  - Python (Google/NumPy/Sphinx docstrings)
  - JavaScript/TypeScript (JSDoc)
  - Go (standard comments)
  - Rust (doc comments)
  - Java
  - C/C++
- Comprehensive documentation:
  - README with quick start guide
  - Technical design document
  - Configuration examples
  - Docker deployment guide

### Security
- API token storage via environment variables
- Webhook signature validation
- Rate limiting considerations
- Sanitized LLM prompts to prevent injection

[Unreleased]: https://github.com/slb350/drep/compare/v1.1.3...HEAD
[1.1.3]: https://github.com/slb350/drep/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/slb350/drep/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/slb350/drep/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/slb350/drep/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/slb350/drep/compare/v0.9.0...v1.0.0
[0.9.0]: https://github.com/slb350/drep/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/slb350/drep/compare/v0.1.0...v0.8.0
[0.1.0]: https://github.com/slb350/drep/releases/tag/v0.1.0
