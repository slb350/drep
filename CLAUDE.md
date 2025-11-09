# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**drep** (PyPI: **drep-ai**) is an AI-powered code review and documentation quality tool that works with **Gitea, GitHub, and GitLab**.

**Key Features:**

1. **AI-Powered Analysis**: LLM-based code quality and documentation review via open-agent-sdk
2. **Documentation Specialist**: Three-tiered approach (dictionary → patterns → LLM) for typos, grammar, and syntax issues
3. **Markdown Linting**: 10 comprehensive checks for documentation quality (opt-in)
4. **Code Quality**: AST parsing and LLM-based detection of bugs and best practices
5. **Automated PR Reviews**: Inline comments on pull requests with actionable feedback
6. **Issue Creation**: Automatically opens issues with detailed findings and suggested fixes
7. **Performance Optimized**: Caching, circuit breakers, metrics, progress tracking

**Current Status:** v0.1.0 Released (2025-10-19)
- ✅ Published on PyPI as `drep-ai`
- ✅ GitHub: https://github.com/slb350/drep
- ✅ 395 tests passing
- ✅ Production-ready

## Architecture

### Core Components

- **Platform Adapters**: Abstraction layer with platform-specific implementations
  - `GiteaAdapter`: Gitea webhook/API integration
  - `GitHubAdapter`: GitHub webhook/API integration
  - `GitLabAdapter`: GitLab webhook/API integration
- **Repository Scanner**: File-by-file analysis using AST parsing and pattern matching
- **Code Analyzer**: Detects bugs, security issues, and best practice violations
- **Documentation Analyzer**: Specialized component for typo/grammar/syntax detection in:
  - Markdown files (*.md)
  - Code comments and docstrings
  - README, CHANGELOG, and documentation files
- **LLM Agent**: Uses open-agent-sdk to power intelligent analysis
- **Issue Manager**: Creates and manages issues with findings (prevents duplicates)
- **PR/MR Reviewer**: Automated review comments on pull requests/merge requests

### Local Development Configuration

**Git Remotes:**
- **Gitea** (origin): `ssh://steve@192.168.1.14:22/steve/drep.git` (local development)
- **GitHub** (github): `git@github.com:slb350/drep.git` (public repository)

**SSH Configuration:**
- Gitea: Default SSH key
- GitHub: `~/.ssh/github_any_agent` (configured via git config)

## Project Status

**Phase:** Released & Maintained
**Version:** 0.1.0
**Release Date:** 2025-10-19
**Last Updated:** 2025-10-19

### ✅ Complete Features
- **Core Implementation**: All phases 1-7 complete
- **LLM Integration**: open-agent-sdk with caching, circuit breakers, metrics
- **Platform Adapters**: Gitea (full support), GitHub/GitLab (API ready)
- **Documentation Analysis**: 10 markdown checks, typo detection, grammar checking
- **Code Analysis**: AST parsing, LLM-based bug detection, docstring generation
- **PR Reviews**: Inline comments, line validation, dual-field Gitea compatibility
- **CLI**: `scan`, `review`, `validate`, `serve`, `metrics` commands
- **Testing**: 395 tests passing (377 unit, 18 integration)
- **CI/CD**: GitHub Actions workflow for automated testing
- **Distribution**: Published on PyPI as `drep-ai`

### 🎯 Next Milestones
- Additional language support (JavaScript/TypeScript, Go, Rust)
- Vector database for cross-file context
- Enhanced metrics dashboard
- Webhook automation improvements

## Documentation

**Public Documentation:**
- **README.md**: Quick start guide, installation, usage examples
- **docs/technical-design.md**: Complete architecture and component design
- **docs/llm-setup.md**: LLM configuration guide (Ollama, LM Studio, llama.cpp)
- **CHANGELOG.md**: Release notes and version history

**Private Documentation (local only):**
- **CLAUDE.md** (this file): Development guidance for Claude Code
- **.gitignore**: Excludes CLAUDE.md, .tokens, .drep/, etc.

## Development Setup

### Prerequisites

- **Python**: 3.10+ (tested on 3.10, 3.11, 3.13)
- **LLM Backend** (optional, for AI features):
  - Ollama, llama.cpp, LM Studio, or any OpenAI-compatible endpoint
  - See `docs/llm-setup.md` for configuration
- **Git**: For repository operations
- **Development**: SSH access to local Gitea (192.168.1.14) and GitHub

### Technology Stack

- **Web Framework**: FastAPI (async webhooks and API)
- **CLI**: Click (command-line interface)
- **Database**: SQLite (file-based, simple)
- **LLM Integration**: open-agent-sdk (multi-backend support)
- **Distribution**: PyPI package + Docker image (hybrid approach)

### Initial Setup

```bash
# Clone repository
git clone git@github.com:slb350/drep.git
cd drep

# Set up Python virtual environment
python -m venv venv

# Install dependencies (use venv/bin/pip)
./venv/bin/pip install -e ".[dev]"

# Run tests to verify setup
./venv/bin/pytest tests/ -v

# Try the CLI
./venv/bin/drep --help
```

**For PyPI Installation** (end users):
```bash
pip install drep-ai
drep --help
```

### Virtual Environment

**IMPORTANT**: This project uses a venv in the project root (`./venv`)

Always use the project venv for Python/pip commands:
```bash
# Install package in development mode
./venv/bin/pip install -e ".[dev]"

# Run tests
./venv/bin/pytest tests/ -v

# Run formatters
./venv/bin/black drep/ tests/
./venv/bin/ruff check drep/ tests/

# Run CLI
./venv/bin/drep --help

# Build package
./venv/bin/python -m build
```

**For Claude Code**: All Python/pip tool uses should reference `./venv/bin/python` or `./venv/bin/pip` explicitly.

## Platform Integration

All platforms (Gitea, GitHub, GitLab) use similar patterns:
1. **REST APIs**: Reading files, creating issues, posting reviews
2. **Webhooks**: Automated triggers for push/PR events (optional)
3. **Authentication**: API tokens via environment variables

### Implemented Adapters

**GiteaAdapter** (fully implemented):
- Multi-version API compatibility (new_position/position fallback)
- Label caching for performance
- Duplicate issue prevention
- Complete PR review support

**GitHubAdapter** (API ready, not yet fully tested)
**GitLabAdapter** (API ready, not yet fully tested)

See `docs/technical-design.md` for detailed adapter specifications.

## Documentation Specialist Implementation

### Tiered Analysis Workflow

```
File received → Layer 1: Dictionary spellcheck
                ↓ (issues found)
              Layer 2: Pattern matching (common errors)
                ↓ (complex/ambiguous)
              Layer 3: LLM analysis with context
                ↓
              Generate Draft PR with fixes
```

### Layer 1: Dictionary Spellcheck
- **Tools**: `pyspellchecker` or similar
- **Scope**: Markdown files, code comments, docstrings
- **Speed**: Instant (< 1ms per file)
- **Catches**: Obvious typos (`recieve` → `receive`, `teh` → `the`)
- **Technical terms**: Maintain custom dictionary of code terms to avoid false positives

### Layer 2: Pattern Matching
- **Method**: Regex patterns for common errors
- **Examples**:
  - Inconsistent capitalization (API vs api)
  - Double spaces, trailing whitespace
  - Broken markdown links `[text](url)` validation
  - Malformed code fences (missing language spec)
- **Speed**: Fast (regex matching)

### Layer 3: LLM Analysis
- **Trigger**: Only for cases requiring context/intelligence
- **Use cases**:
  - Ambiguous style decisions (is this term capitalized in project?)
  - Grammar complexity (sentence structure, clarity)
  - Technical accuracy validation
  - Comment quality assessment
  - Missing comment generation

### Comment Quality Features

#### Missing Comments Detection
Target code elements needing documentation:
- **Python**: Functions/methods without docstrings, classes > 2 methods
- **JavaScript/TypeScript**: Functions without JSDoc, exported APIs
- **Go**: Exported functions without standard comments
- **Rust**: Public items without doc comments
- **Threshold**: Only flag functions > 10 lines, public APIs, complex logic

#### Bad Comment Detection
Flag low-quality comments:
- Generic/obvious: `# this function does stuff`
- Outdated: Comment contradicts code behavior (LLM compares)
- Redundant: `i += 1  # increment i`
- TODOs without context: `# TODO: fix this`

#### Comment Generation
LLM generates documentation following language conventions:
- **Python**: Google/NumPy/Sphinx style docstrings
- **JavaScript**: JSDoc with `@param`, `@returns`
- **Go**: Standard comment format
- **Rust**: Triple-slash doc comments with examples

### Draft PR Workflow

All documentation fixes → Single draft PR per scan:
- Branch naming: `drep/docs-fixes-YYYY-MM-DD-HASH`
- PR title: `[drep] Documentation improvements`
- PR body: Categorized list of changes:
  - 🔤 Typo fixes
  - 📝 Missing comments added
  - ✨ Comment quality improvements
  - 📖 Markdown/formatting fixes
- Marked as **draft** for human review before merge
- Label: `documentation`, `automated`

### File Targeting

**Documentation files:**
- `*.md` (README, CHANGELOG, docs/)
- `*.rst` (ReStructuredText)
- `*.txt` (LICENSE, CONTRIBUTING)

**Code comments:**
- Python: `*.py` (docstrings, inline comments)
- JavaScript/TypeScript: `*.js`, `*.ts`, `*.jsx`, `*.tsx`
- Go: `*.go`
- Rust: `*.rs`
- Java: `*.java`
- C/C++: `*.c`, `*.cpp`, `*.h`, `*.hpp`

## Testing

**Test Suite**: 395 tests (all passing)
- **Unit Tests**: 377 tests covering core functionality
- **Integration Tests**: 18 tests for external service interactions

**Key Test Coverage:**
- Platform adapters (Gitea, GitHub, GitLab)
- LLM client with circuit breakers and caching
- Documentation analysis (10 markdown checks)
- Code analysis (AST parsing, docstring generation)
- PR review workflow (line validation, comment posting)
- CLI commands (scan, review, validate, serve, metrics)
- Performance components (ProgressTracker, ParallelAnalyzer)
- Metrics and observability

**Running Tests:**
```bash
# All tests (excluding integration)
./venv/bin/pytest tests/ -v -k "not integration"

# With coverage
./venv/bin/pytest tests/ --cov=drep --cov-report=html

# Specific test file
./venv/bin/pytest tests/unit/test_markdown_analyzer.py -v

# Integration tests only
./venv/bin/pytest tests/ -v -m integration
```

## Key Design Considerations

1. **Platform Abstraction**: Common interface for all git platforms with adapter pattern
2. **File-by-File Analysis**: Each file analyzed independently
   - Simpler implementation, lower memory footprint
   - Effective for: linting, documentation errors, common patterns
   - Future enhancement: Vector DB for cross-file context
3. **Tiered Documentation Analysis**:
   - Layer 1: Dictionary spellcheck (instant)
   - Layer 2: Pattern matching (10 markdown checks)
   - Layer 3: LLM analysis (complex cases)
4. **Performance Optimization**:
   - IntelligentCache with 80%+ hit rates
   - Circuit breaker pattern for LLM resilience
   - Progress tracking with real-time updates
   - Parallel analysis where applicable
5. **Quality Assurance**:
   - Issue deduplication via SQLite cache
   - Line number validation for PR comments
   - Multi-version platform compatibility (Gitea new_position/position fallback)
6. **Observability**:
   - Comprehensive metrics (tokens, costs, per-analyzer breakdown)
   - Structured logging with context
   - Export to JSON for analysis

## Release Process

### Quick Reference

**PyPI Package**:
- **Package Name**: `drep-ai` (PyPI), `drep` (GitHub repo)
- **Install**: `pip install drep-ai`
- **Import**: `from drep import ...` (module name unchanged)
- **PyPI URL**: https://pypi.org/project/drep-ai/
- **TestPyPI URL**: https://test.pypi.org/project/drep-ai/

**Why drep-ai?**
- Original `drep` name taken on PyPI by bioinformatics project
- `drep-ai` highlights AI/LLM-powered capabilities (key differentiator)
- Better marketing appeal and shows it's not just rule-based linting

**Git Remotes**:
- **GitHub**: `git@github.com:slb350/drep.git`
- **Gitea**: `ssh://steve@192.168.1.14:22/steve/drep.git`

**Pushing to GitHub**:
```bash
# Use the specific SSH key for GitHub (same as open-agent-sdk)
GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent' git push github main

# Or configure permanently for this repo:
git config core.sshCommand "ssh -i ~/.ssh/github_any_agent"
```

### Complete Release Checklist

Complete checklist for releasing a new version (e.g., v0.2.0):

#### 1. Development on Feature Branch

```bash
# Create and work on feature branch
git checkout -b feature-name

# Make your changes, then update documentation:
```

**Update these files**:
- ✅ `CHANGELOG.md` - Add new version entry at top with date (YYYY-MM-DD format)
- ✅ `README.md` - Update examples, API reference if needed
- ✅ `docs/technical-design.md` - Update architecture details if changed
- ✅ `pyproject.toml` - Bump version number (remember: PyPI name is `drep-ai`)
- ✅ `docs/llm-setup.md` - Update if LLM integration changes
- ✅ Examples in `examples/` - Update if API changed

**IMPORTANT**: Package name on PyPI is `drep-ai`, not `drep`
- pyproject.toml: `name = "drep-ai"`
- Installation: `pip install drep-ai`
- Import path: `from drep import ...` (unchanged)

**Verify quality**:
```bash
# Run all tests (use venv)
./venv/bin/pytest tests/ -v

# Run linters (use venv)
./venv/bin/ruff check drep/ tests/
./venv/bin/black --check drep/ tests/

# Fix any issues (use venv)
./venv/bin/ruff check drep/ tests/ --fix
./venv/bin/black drep/ tests/
```

**Push feature branch**:
```bash
# Push to Gitea (origin) for backup/review
git add .
git commit -m "feat: descriptive message"
git push origin feature-name
```

#### 2. Merge to Main

```bash
# Switch to main and merge
git checkout main
git merge feature-name --no-ff

# Push to Gitea
git push origin main
```

#### 3. Build and Release

**Build package**:
```bash
# Clean and build (use venv)
rm -rf dist/
./venv/bin/python -m build
```

**Add GitHub remote (first time only)**:
```bash
# Add GitHub remote
git remote add github git@github.com:slb350/drep.git

# Configure SSH key for this repo
git config core.sshCommand "ssh -i ~/.ssh/github_any_agent"
```

**Push to GitHub**:
```bash
# Push main branch to public GitHub repo
GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent' git push github main
```

**Create and push git tag**:
```bash
# Create version tag
git tag v0.2.0

# Push tag to both remotes
git push origin v0.2.0
GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent' git push github v0.2.0
```

**Upload to PyPI**:

**⚠️ CRITICAL: NEVER COMMIT .tokens FILE TO GIT**
- The `.tokens` file contains PyPI authentication tokens
- It is listed in `.gitignore` and must stay that way
- If you accidentally commit it, immediately revoke tokens and generate new ones
- Copied from `~/Dev/any-agent/.tokens` to local `.tokens` file

```bash
# Source tokens from local .tokens file (NEVER COMMIT THIS FILE)
source .tokens

# Upload to TestPyPI first (verify it works) - use venv
./venv/bin/twine upload --repository testpypi dist/* \
  --username __token__ --password $TESTPYPI_TOKEN

# Upload to Production PyPI - use venv
./venv/bin/twine upload dist/* \
  --username __token__ --password $PYPI_TOKEN
```

**Create GitHub Release**:
```bash
# Create release from tag with CHANGELOG content
gh release create v0.2.0 \
  --title "v0.2.0 - Feature Name" \
  --notes "Copy the relevant section from CHANGELOG.md here"
```

#### 4. Verify Release

```bash
# Check TestPyPI
open https://test.pypi.org/project/drep-ai/0.2.0/

# Check Production PyPI
open https://pypi.org/project/drep-ai/0.2.0/

# Check GitHub tag
open https://github.com/slb350/drep/releases

# Test installation from PyPI (can use system pip for testing)
pip install --upgrade drep-ai
drep --version

# Or test in venv
./venv/bin/pip install --upgrade drep-ai
./venv/bin/drep --version
```

#### 5. Cleanup (Optional)

```bash
# Delete feature branch locally
git branch -d feature-name

# Delete feature branch on Gitea
git push origin --delete feature-name
```

### Common Issues

**License warnings during build**:
- Setuptools deprecation warnings about `project.license` format
- These are harmless but can be fixed later by updating pyproject.toml format

**Git push authentication**:
- Always use `GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent'` for GitHub
- Gitea (origin) uses default SSH key
- Or configure permanently: `git config core.sshCommand "ssh -i ~/.ssh/github_any_agent"`

**PyPI upload failures**:
- Verify tokens are sourced: `echo $TESTPYPI_TOKEN` (should show token)
- Check package name isn't already taken
- Verify version number is unique (can't re-upload same version)
- Ensure `build` and `twine` are installed: `pip install build twine`

**CHANGELOG URLs**:
- GitHub URLs already configured with username `slb350`
- URLs will work once repository is created on GitHub

### Important Notes

**When NOT on main branch**:
- Only push to `origin` (Gitea), NOT GitHub
- GitHub is for PyPI releases and public visibility
- Only push to GitHub from `main` branch

**Token Management**:
- `.tokens` file copied from `~/Dev/any-agent/.tokens` to local directory
- Contains `PYPI_TOKEN` and `TESTPYPI_TOKEN` variables
- NEVER commit this file to git
- Already in `.gitignore` (verified)
- Source with: `source .tokens`

**Version Numbering**:
- Follow Semantic Versioning (SemVer)
- Format: MAJOR.MINOR.PATCH (e.g., 0.2.0)
- Increment MAJOR for breaking changes
- Increment MINOR for new features
- Increment PATCH for bug fixes

**Package Naming**:
- **PyPI Package**: `drep-ai` (use in pyproject.toml `name` field)
- **GitHub Repo**: `drep` (repository name)
- **Python Module**: `drep` (import path, directory name)
- **CLI Command**: `drep` (executable name)

This naming scheme keeps the clean "drep" branding while avoiding PyPI conflicts.
