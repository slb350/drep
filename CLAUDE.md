# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**drep** is a platform-agnostic code review automation tool that works with **Gitea, GitHub, and GitLab**. Similar to Greptile but more proactive, it:

1. **Code Quality**: Continuously scans repositories for code issues, bugs, and best practice violations
2. **Documentation Specialist**: Detects and fixes typos, grammar errors, and syntax issues in documentation, comments, and docstrings
3. **Issue Creation**: Automatically opens issues with detailed findings and suggested fixes
4. **PR/MR Reviews**: Conducts automated code reviews when pull requests or merge requests are opened
5. **Local LLM**: Powered by local LLM via the open-agent-sdk (https://pypi.org/project/open-agent-sdk/)

**MVP Scope**:
- **Platform:** Gitea only (self-hosted at 192.168.1.14)
- **Language:** Python only
- **Analysis:** File-by-file (no vector database, no LLM yet)
- **Features:** Typo detection + pattern matching + issue creation
- **Scanning:** Manual `drep scan` command with incremental diffs

**Post-MVP Expansions:**
- Phase 2: LLM integration, draft PRs, Gitea webhooks
- Phase 3: JavaScript/TypeScript support
- Phase 4: GitHub and GitLab adapters
- Phase 5: Vector database, Go/Rust support

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

### Gitea Server Configuration

- **Server**: Self-hosted at 192.168.1.14
- **Username**: steve
- **Git remote format**: `ssh://steve@192.168.1.14:22/steve/drep.git`
- **Authentication**: SSH key already configured

## Project Status

**Phase:** Design Complete, Ready for Implementation
**Version:** 0.1.0 (MVP in progress)
**Last Updated:** 2025-10-17

### What's Ready
- ✅ Complete technical design (Gitea + Python only MVP)
- ✅ Detailed implementation plan with step-by-step TODOs
- ✅ All security issues resolved (git authentication via GIT_ASKPASS)
- ✅ Project scaffolding complete (directories, pyproject.toml, etc.)

### Next Steps
Start implementation at Phase 1, Task 1.1 in `docs/implementation-plan.md`

## Documentation

- **Technical Design**: See `docs/technical-design.md` for complete architecture, component design, and implementation-ready code examples
- **Implementation Plan**: See `docs/implementation-plan.md` for detailed step-by-step TODOs, testing guidelines, and success criteria

## Development Setup

### Prerequisites

- Python 3.10+
- Access to local LLM (for open-agent-sdk)
  - Supports: Ollama, llama.cpp, LM Studio (OpenAI-compatible endpoints)
- SSH access to Gitea server at 192.168.1.14
- open-agent-sdk: `pip install open-agent-sdk`

### Technology Stack

- **Web Framework**: FastAPI (async webhooks and API)
- **CLI**: Click (command-line interface)
- **Database**: SQLite (file-based, simple)
- **LLM Integration**: open-agent-sdk (multi-backend support)
- **Distribution**: PyPI package + Docker image (hybrid approach)

### Initial Setup

```bash
# Initialize git repository
git init
git remote add origin ssh://steve@192.168.1.14:22/steve/drep.git

# Install dependencies
pip install open-agent-sdk

# Set up Python virtual environment
python -m venv venv
source venv/bin/activate  # or `venv/bin/activate` on Unix
```

## Platform Integration

### Architecture Approach

All platforms (Gitea, GitHub, GitLab) support similar integration patterns:
1. **Webhooks**: External service receiving webhook events (PR/MR opened, push, etc.)
2. **REST APIs**: API endpoints for reading files, creating issues, posting reviews
3. **External Service**: Standalone service that polls or responds to webhooks

**drep** uses webhooks + REST APIs to monitor repositories and create issues/reviews.

### Platform-Specific Configuration

#### Gitea (Primary Development Target)
- API endpoint: `http://192.168.1.14:3000/api/v1/`
- Authentication: API token or SSH key
- Key endpoints:
  - `/repos/{owner}/{repo}/issues` - Create issues
  - `/repos/{owner}/{repo}/pulls/{index}/reviews` - Submit PR reviews
  - `/repos/{owner}/{repo}/contents/{filepath}` - Read file contents

#### GitHub
- API endpoint: `https://api.github.com`
- Authentication: Personal Access Token or GitHub App
- Webhook events: `pull_request`, `push`, `issues`

#### GitLab
- API endpoint: `https://gitlab.com/api/v4` (or self-hosted URL)
- Authentication: Personal Access Token or OAuth
- Webhook events: `merge_request`, `push`, `issue`

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

## Key Design Considerations

1. **Platform Abstraction**: Common interface for all git platforms with adapter pattern
2. **File-by-File Analysis (MVP)**: Each file analyzed independently without cross-file context
   - Simpler implementation, lower memory footprint
   - Effective for: linting, documentation errors, common patterns
   - Limitation: Cannot detect cross-file dependencies or architectural issues
3. **Documentation Analysis**: Specialized handling for documentation files
   - **Tiered hybrid approach** for grammar/style checking:
     - Layer 1: Dictionary spellcheck (instant, catches obvious typos)
     - Layer 2: Pattern matching for common errors (regex-based)
     - Layer 3: LLM analysis for complex/ambiguous cases
   - Markdown syntax validation
   - Consistency checks (capitalization, formatting, style)
   - Code comment quality assessment:
     - Detect missing docstrings/comments on public APIs, complex functions
     - Identify low-quality comments (e.g., "# this function does stuff")
     - Generate suggested improvements using LLM
   - **Draft PR creation** for all documentation fixes (typos, missing comments, improvements)
4. **Scanning Strategy**: Balance between continuous monitoring and resource usage
5. **Issue Deduplication**: Track findings to avoid creating duplicate issues
6. **Review Quality**: Ensure automated reviews are actionable and non-noisy
7. **Future: Vector DB**: Post-release feature for codebase-wide context and semantic search
