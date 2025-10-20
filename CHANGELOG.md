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

[Unreleased]: https://github.com/slb350/drep/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/slb350/drep/releases/tag/v0.1.0
