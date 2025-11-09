# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- Vector database integration for cross-file context
- Custom rule definitions
- Integration with existing linters (pylint, eslint, etc.)
- Metrics dashboard
- Notification system (Slack, Discord)
- Multi-repository analysis features
- Anthropic Direct API provider (Phase 3.4)

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

[Unreleased]: https://github.com/slb350/drep/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/slb350/drep/compare/v0.9.0...v1.0.0
[0.9.0]: https://github.com/slb350/drep/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/slb350/drep/compare/v0.1.0...v0.8.0
[0.1.0]: https://github.com/slb350/drep/releases/tag/v0.1.0
