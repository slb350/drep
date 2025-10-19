# drep

**D**ocumentation & **R**eview **E**nhancement **P**latform

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)

Automated code review and documentation improvement tool for **Gitea**, **GitHub**, and **GitLab**. Powered by local LLM via [open-agent-sdk](https://pypi.org/project/open-agent-sdk/).

> **MVP Status:** Currently supports **Gitea** with **Python** repositories. GitHub/GitLab and additional languages coming in future releases.

## Features

### Proactive Code Analysis
Unlike reactive tools, drep continuously monitors repositories and automatically:
- Detects bugs, security vulnerabilities, and best practice violations
- Opens issues with detailed findings and suggested fixes
- No manual intervention required

### Documentation Specialist
Three-tiered analysis for comprehensive documentation quality:
- **Layer 1**: Dictionary spellcheck (instant typo detection)
- **Layer 2**: Pattern matching (formatting, syntax, consistency)
- **Layer 3**: LLM analysis (grammar, clarity, technical accuracy)

**Automated improvements for:**
- Typos and grammar errors in markdown, comments, and docstrings
- Missing documentation on functions, methods, and classes
- Low-quality comments (generic, outdated, or redundant)
- Markdown syntax issues (broken links, malformed tables)

### Automated PR/MR Reviews
Intelligent code review when pull requests or merge requests are opened:
- Analyzes changed files for issues
- Posts line-specific review comments
- Suggests improvements with explanations

### Draft PR Creation
All documentation fixes are automatically applied and submitted as draft PRs:
- Categorized changes (typos, missing comments, improvements)
- Human review before merge
- Non-intrusive workflow

### Local LLM Powered
Complete privacy and control:
- Uses your local LLM (Ollama, llama.cpp, LM Studio)
- No external API calls
- No cloud dependencies
- No usage costs

### Platform Agnostic
Single tool for all your git platforms:
- **Gitea** (self-hosted)
- **GitHub**
- **GitLab** (cloud or self-hosted)

## LLM-Powered Analysis

drep includes intelligent code analysis powered by local LLMs via LM Studio.

### Features

- **Code Quality Analysis**: Detects bugs, security issues, and best practice violations
- **Docstring Generation**: Automatically generates Google-style docstrings
- **PR Reviews**: Context-aware code review comments
- **Smart Caching**: 80%+ cache hit rate on repeated scans
- **Cost Tracking**: Monitor token usage and estimated costs
- **Circuit Breaker**: Graceful degradation when LLM unavailable
- **Progress Reporting**: Real-time feedback during analysis

### Quick Start

1. Install LM Studio: https://lmstudio.ai/
2. Download a model (Qwen3-30B-A3B recommended)
3. Configure drep:

```yaml
llm:
  enabled: true
  endpoint: http://localhost:1234/v1
  model: qwen3-30b-a3b
  temperature: 0.2
  max_tokens: 8000

  # Rate limiting
  max_concurrent_global: 5
  requests_per_minute: 60

  # Caching
  cache:
    enabled: true
    ttl_days: 30
```

4. Run analysis:

```bash
drep scan owner/repo --show-progress --show-metrics
```

### View Metrics

```bash
# Show detailed usage statistics
drep metrics --detailed

# Export to JSON
drep metrics --export metrics.json

# Last 7 days only
drep metrics --days 7
```

**Example output:**
```
===== LLM Usage Report =====
Session duration: 0h 5m 32s
Total requests: 127 (115 successful, 12 failed, 95 cached)
Success rate: 90.6%
Cache hit rate: 74.8%

Tokens used: 45,230 prompt + 12,560 completion = 57,790 total
Estimated cost: $0.29 USD (or $0 with LM Studio)

Performance:
  Average latency: 1250ms
  Min/Max: 450ms / 3200ms

By Analyzer:
  code_quality: 45 requests (12,345 tokens)
  docstring: 38 requests (8,901 tokens)
  pr_review: 44 requests (36,544 tokens)
```

## Quick Start

### Installation

#### Via pip (Recommended)
```bash
pip install drep
```

#### Via Docker
```bash
docker pull ghcr.io/stephenbrandon/drep:latest
```

### Configuration

```bash
# Initialize configuration
drep init

# Edit config.yaml with your platform credentials
vim config.yaml
```

**Minimal config.yaml:**
```yaml
platforms:
  - type: gitea
    url: http://localhost:3000
    token: your-gitea-token
    repositories:
      - owner/*  # Monitor all repos for this owner

llm:
  endpoint: http://localhost:11434  # Ollama endpoint
  model: llama3.2

documentation:
  enabled: true
  create_draft_prs: true

code_analysis:
  enabled: true
```

### Run drep

#### As a Service (Recommended)
```bash
# Start web server to receive webhooks
drep serve --host 0.0.0.0 --port 8000
```

Configure webhooks in your git platform to point to:
- Gitea: `http://your-server:8000/webhooks/gitea`
- GitHub: `http://your-server:8000/webhooks/github`
- GitLab: `http://your-server:8000/webhooks/gitlab`

#### Manual Scan
```bash
# Scan a specific repository
drep scan owner/repository --platform gitea
```

#### Docker Compose (with Ollama)
```yaml
version: '3.8'
services:
  drep:
    image: ghcr.io/stephenbrandon/drep:latest
    ports:
      - "8000:8000"
    volumes:
      - ./config.yaml:/app/config.yaml
      - ./data:/app/data
    environment:
      - DREP_LLM_ENDPOINT=http://ollama:11434
    depends_on:
      - ollama

  ollama:
    image: ollama/ollama:latest
    ports:
      - "11434:11434"
    volumes:
      - ollama_data:/root/.ollama

volumes:
  ollama_data:
```

```bash
docker compose up -d
```

## How It Works

### Repository Scanning
```
Push Event → drep receives webhook
           ↓
         Scans all files
           ↓
   ┌──────┴──────┐
   ▼             ▼
Doc Analysis  Code Analysis
   ↓             ↓
Draft PR      Issues Created
```

### Documentation Analysis
```
File → Layer 1: Spellcheck (instant)
       ↓
     Layer 2: Pattern matching (regex)
       ↓
     Layer 3: LLM analysis (complex cases)
       ↓
     Draft PR with fixes
```

### PR Review
```
PR Opened → Analyze changed files
           ↓
         Find issues
           ↓
    Post review comments
```

## What drep Detects

### Documentation Issues
- Typos and spelling errors
- Grammar and sentence structure
- Inconsistent capitalization/formatting
- Broken markdown links
- Missing code fence language specifications
- Functions without docstrings
- Generic comments ("this function does stuff")
- Outdated comments (contradicting code)
- Redundant comments ("i += 1  # increment i")

### Code Issues
- Bare except clauses
- Mutable default arguments
- Security vulnerabilities
- Best practice violations
- Potential bugs
- Performance issues

### Supported Languages
- Python (Google/NumPy/Sphinx docstrings)
- JavaScript/TypeScript (JSDoc)
- Go (standard comments)
- Rust (doc comments)
- Java
- C/C++

## Example Output

### Draft PR Created by drep

```markdown
# [drep] Documentation improvements

## Typo Fixes (12)
- README.md:15: recieve → receive
- docs/setup.md:42: teh → the

## Missing Comments Added (5)
- api/handlers.py:78: Added docstring to create_user method

## Comment Quality Improvements (3)
- utils/helpers.py:45: Improved "does stuff" → "Validates user input format"

## Markdown Formatting (8)
- CHANGELOG.md:22: Added language spec to code fence
```

## Configuration

### Full config.yaml Example

```yaml
# Platform configurations
platforms:
  - type: gitea
    url: http://192.168.1.14:3000
    token: ${GITEA_TOKEN}
    repositories:
      - steve/*

  - type: github
    token: ${GITHUB_TOKEN}
    repositories:
      - myorg/repo1
      - myorg/repo2

  - type: gitlab
    url: https://gitlab.com  # Optional for cloud GitLab
    token: ${GITLAB_TOKEN}
    repositories:
      - mygroup/project1

# LLM configuration
llm:
  endpoint: http://localhost:11434
  model: llama3.2
  temperature: 0.3
  timeout: 120

# Documentation analysis
documentation:
  enabled: true
  create_draft_prs: true
  languages:
    - python
    - javascript
    - typescript
    - go
    - rust
  custom_dictionary:
    - asyncio
    - fastapi
    - kubernetes

# Code analysis
code_analysis:
  enabled: true
  security_checks: true
  best_practices: true
  create_issues: true

# Scanning configuration
scan:
  interval: 3600  # Scan every hour
  on_push: true   # Also scan on git push
```

### Environment Variables

```bash
# Platform tokens (recommended over hardcoding)
export GITEA_TOKEN="your-token"
export GITHUB_TOKEN="your-token"
export GITLAB_TOKEN="your-token"

# Override config file location
export DREP_CONFIG="/path/to/config.yaml"

# Override LLM endpoint
export DREP_LLM_ENDPOINT="http://localhost:11434"
```

## CLI Commands

```bash
# Initialize configuration
drep init [--config config.yaml]

# Validate configuration
drep validate [--config config.yaml]

# Start web server
drep serve [--host 0.0.0.0] [--port 8000] [--config config.yaml]

# Manual repository scan
drep scan owner/repo [--platform gitea] [--config config.yaml]
```

## Architecture

drep uses a modular architecture with platform adapters:

```
drep/
├── adapters/         # Platform-specific implementations
│   ├── base.py       # Abstract adapter interface
│   ├── gitea.py      # Gitea adapter
│   ├── github.py     # GitHub adapter
│   └── gitlab.py     # GitLab adapter
├── core/             # Core business logic
├── documentation/    # Documentation analyzer
└── models/           # Data models
```

See [docs/technical-design.md](docs/technical-design.md) for complete architecture details.

## Development

### Setup Development Environment

```bash
# Clone repository
git clone ssh://steve@192.168.1.14:22/steve/drep.git
cd drep

# Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install in development mode
pip install -e ".[dev]"

# Run tests
pytest

# Format code
black drep/
ruff check drep/

# Type checking
mypy drep/
```

### Running Tests

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=drep --cov-report=html

# Run specific test file
pytest tests/unit/test_adapters.py
```

## Roadmap

### MVP (Current)
- ✅ Platform adapters (Gitea, GitHub, GitLab)
- ✅ Documentation analyzer (3-layer)
- ✅ Code analyzer (AST + LLM)
- ✅ Draft PR creation
- ✅ Webhook server
- ✅ SQLite database
- ✅ CLI interface

### Post-MVP
- [ ] Vector database integration (cross-file context)
- [ ] Custom rule definitions
- [ ] Integration with existing linters
- [ ] Metrics dashboard
- [ ] Notification system (Slack, Discord)
- [ ] Multi-repository analysis

## Comparison with Existing Tools

| Feature | drep | Greptile | PR-Agent | Codedog |
|---------|------|----------|----------|---------|
| **Proactive Scanning** | ✅ | ❌ | ❌ | ❌ |
| **Documentation Specialist** | ✅ | ❌ | ❌ | ❌ |
| **Draft PR Creation** | ✅ | ❌ | ❌ | ❌ |
| **PR Reviews** | ✅ | ❌ | ✅ | ✅ |
| **Local LLM** | ✅ | ❌ | Partial | Partial |
| **Multi-Platform** | ✅ | ✅ | ✅ | ✅ |
| **Self-Hosted** | ✅ | ❌ | ✅ | ✅ |

**Key Differentiator**: drep is the only tool that proactively scans repositories, specializes in documentation, and automatically creates draft PRs with fixes.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Support

- **Documentation**: [docs/](docs/)
- **Issues**: https://github.com/stephenbrandon/drep/issues
- **Discussions**: https://github.com/stephenbrandon/drep/discussions

## Acknowledgments

- Built with [open-agent-sdk](https://pypi.org/project/open-agent-sdk/)
- Inspired by tools like Greptile, PR-Agent, and Codedog
- Thanks to the open-source community

---

**Made with ❤️ for developers who care about code quality and documentation**
