# drep

**D**ocumentation & **R**eview **E**nhancement **P**latform

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)

Automated code review and documentation improvement tool for **Gitea** (initial release). Powered by your local LLM via an OpenAI-compatible API (LM Studio, Ollama, open-agent-sdk).

> **Initial Release Scope:** Python repositories on Gitea. Support for GitHub, GitLab, and additional languages is in active development.

## Features

### Proactive Code Analysis
Unlike reactive tools, drep continuously monitors repositories and automatically:
- Detects bugs, security vulnerabilities, and best practice violations
- Opens issues with detailed findings and suggested fixes
- No manual intervention required

### Docstring Intelligence
LLM-powered docstring analysis purpose-built for Python:
- Generates Google-style docstrings for public APIs
- Flags TODOs, placeholders, and low-signal docstrings
- Respects decorators (e.g., `@property`, `@classmethod`) and skips simple helpers

### Automated PR/MR Reviews
Intelligent review workflow for Gitea pull requests:
- Parses diffs into structured hunks
- Generates inline comments tied to added lines
- Produces a high-level summary with approval signal

### Local LLM Powered
Complete privacy and control:
- Uses your local LLM (Ollama, llama.cpp, LM Studio)
- No external API calls
- No cloud dependencies
- No usage costs

### Platform Support & Roadmap
- **Available now:** Gitea + Python repositories
- **Planned:** GitHub, GitLab, additional languages, advanced draft PR workflows

## LLM-Powered Analysis

drep includes intelligent code analysis powered by local LLMs via OpenAI-compatible backends (LM Studio, Ollama, open-agent-sdk).

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
  endpoint: http://localhost:1234/v1  # LM Studio / OpenAI-compatible API (also works with open-agent-sdk)
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

#### From source (until PyPI release)
```bash
git clone https://github.com/stephenbrandon/drep.git
cd drep
pip install -e ".[dev]"
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
gitea:
  url: http://localhost:3000
  token: ${GITEA_TOKEN}
  repositories:
    - owner/*  # Monitor all repos for this owner

documentation:
  enabled: true
  custom_dictionary: []
  # Enable lightweight Markdown checks (non-LLM)
  markdown_checks: false

database_url: sqlite:///./drep.db

llm:
  enabled: true
  endpoint: http://localhost:1234/v1  # LM Studio (OpenAI-compatible)
  # Or for Ollama's OpenAI-compatible API: http://localhost:11434/v1
  model: qwen3-30b-a3b
  temperature: 0.2
  max_tokens: 8000
  cache:
    enabled: true
    ttl_days: 30
```

### Run drep

#### As a Service (Recommended)
```bash
# Start web server to receive webhooks
drep serve --host 0.0.0.0 --port 8000
```

Configure Gitea webhooks to point to:
- Gitea: `http://your-server:8000/webhooks/gitea`

#### Manual Scan
```bash
# Scan a specific repository
drep scan owner/repository
```

#### Review a Pull Request
```bash
# Analyze PR #42 on owner/repository without posting comments
drep review owner/repository 42 --no-post
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
Doc Analysis        Code Analysis
   ↓                    ↓
Docstring Findings   Code Quality Findings
           ↘          ↙
         Issues / Review Comments
```

### Docstring Analysis (Python)
```
File → Function extraction → Filtering (public ≥3 lines) → LLM docstring review
                                                    ↓
                                          Suggestions & findings
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
- Missing docstrings on public functions and methods
- Placeholder docstrings containing TODO/FIXME text
- Generic descriptions that fail to explain purpose or behavior
- Decorated accessors without documentation (`@property`, `@classmethod`)
 - Optional Markdown checks (when `documentation.markdown_checks` = true):
   - Trailing whitespace, tabs
   - Empty or malformed headings (e.g., missing space after `#`)
   - Unclosed code fences (```)
   - Long lines (>120 chars), multiple blank lines, trailing blank lines
   - Bare URLs (suggest wrapping in `[text](url)`) and basic broken link syntax

### Code Issues
- Bare except clauses
- Mutable default arguments
- Security vulnerabilities
- Best practice violations
- Potential bugs
- Performance issues

### Supported Languages
- Python (Google-style docstrings)

*Additional language support is planned for upcoming releases.*

## Example Output

### Example PR Review Summary

```markdown
## 🤖 drep AI Code Review

Looks great overall! Tests cover the new behavior and naming is clear.

**Recommendation:** ✅ Approve

---
*Generated by drep using qwen3-30b-a3b*
```

### Example Docstring Suggestion

````markdown
Suggested docstring for `calculate_total()`:

```python
def calculate_total(...):
    """
    Compute the final invoice total including tax.

    Args:
        prices: Individual line-item amounts.
        tax_rate: Tax rate expressed as a decimal.

    Returns:
        Total amount with tax applied.
    """
```

**Reasoning:** Summarizes the calculation inputs and highlights tax handling.
````

## Configuration

### Full config.yaml Example

```yaml
gitea:
  url: http://localhost:3000
  token: ${GITEA_TOKEN}
  repositories:
    - your-org/*

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - fastapi
    - kubernetes

database_url: sqlite:///./drep.db

llm:
  enabled: true
  endpoint: http://localhost:1234/v1  # LM Studio / Ollama endpoint
  model: qwen3-30b-a3b
  temperature: 0.2
  timeout: 120
  max_retries: 3
  retry_delay: 2
  max_concurrent_global: 5
  max_concurrent_per_repo: 3
  requests_per_minute: 60
  max_tokens_per_minute: 80000
  cache:
    enabled: true
    directory: ~/.cache/drep/llm
    ttl_days: 30
    max_size_gb: 10
```

### Environment Variables

```bash
# Platform tokens (recommended over hardcoding)
export GITEA_TOKEN="your-token"
# Future adapters will also respect:
# export GITHUB_TOKEN="your-token"
# export GITLAB_TOKEN="your-token"

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
drep serve [--host 0.0.0.0] [--port 8000]

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
git clone https://github.com/stephenbrandon/drep.git
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
- ✅ Gitea adapter
- ✅ LLM-powered code quality analyzer
- ✅ Docstring generator for Python
- ✅ PR review CLI workflow
- ✅ SQLite database
- ✅ CLI interface

### Post-MVP
- [ ] GitHub and GitLab adapters
- [ ] Draft PR automation
- [ ] Vector database integration (cross-file context)
- [ ] Custom rule definitions
- [ ] Integration with existing linters
- [ ] Metrics dashboard
- [ ] Notification system (Slack, Discord)
- [ ] Multi-language docstring and code support
- [ ] Multi-repository analysis

## Comparison with Existing Tools

| Feature | drep (current) | Greptile | PR-Agent | Codedog |
|---------|----------------|----------|----------|---------|
| **CLI repository scans** | ✅ | ❌ | ❌ | ❌ |
| **Docstring suggestions (Python)** | ✅ | ❌ | ❌ | ❌ |
| **Gitea PR reviews** | ✅ | ❌ | ❌ | ❌ |
| **Local LLM** | ✅ | ❌ | Partial | Partial |
| **Gitea support** | ✅ | ❌ | ❌ | ❌ |
| **Draft PR automation** | 🚧 Planned | ❌ | ❌ | ❌ |
| **GitHub/GitLab support** | 🚧 Planned | ✅ | ✅ | ✅ |

**Key Differentiator**: drep focuses on local, privacy-preserving analysis with docstring intelligence and PR reviews powered by your own LLM. Broader platform and language support is in progress.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Support

- **Documentation**: [docs/](docs/)
- **Issues**: https://github.com/stephenbrandon/drep/issues
- **Discussions**: https://github.com/stephenbrandon/drep/discussions

## Acknowledgments

- Uses OpenAI-compatible local LLMs (LM Studio, Ollama)
- Inspired by tools like Greptile, PR-Agent, and Codedog
- Thanks to the open-source community

---

**Made with ❤️ for developers who care about code quality and documentation**
