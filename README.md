# drep

**D**ocumentation & **R**eview **E**nhancement **P**latform

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)

Automated code review and documentation improvement tool for **Gitea and GitHub**. Powered by your choice of LLM backend: local models (LM Studio, Ollama, llama.cpp), AWS Bedrock, or Anthropic's Claude API.

> **Current Scope:** Python repositories on Gitea and GitHub. Support for GitLab, additional languages, and AWS Bedrock/Anthropic providers is in active development.

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

### Flexible LLM Backends
Choose the right LLM backend for your needs:
- **Local models:** Complete privacy with Ollama, llama.cpp, LM Studio
- **AWS Bedrock:** Enterprise compliance with Claude on AWS (planned)
- **Anthropic Direct:** Latest Claude models with direct API access (planned)
- **OpenAI-compatible:** Works with any compatible endpoint

### Platform Support & Roadmap
- **Available now:** Gitea, GitHub + Python repositories
- **Planned:** GitLab, additional languages, advanced draft PR workflows

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
pip install drep-ai
```

**Note**: The PyPI package is named `drep-ai` (the name `drep` was already taken). After installation, the command-line tool is still `drep`.

#### From source
```bash
git clone https://github.com/slb350/drep.git
cd drep
pip install -e ".[dev]"
```

#### Via Docker
```bash
docker pull ghcr.io/slb350/drep:latest
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
    image: ghcr.io/slb350/drep:latest
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

**Option 1: Local LLM (LM Studio / Ollama)**
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

**Option 2: AWS Bedrock (Planned - Phase 3.3)**
```yaml
llm:
  enabled: true
  provider: bedrock

  bedrock:
    region: us-east-1
    model: anthropic.claude-3-5-sonnet-20241022-v2:0
    # Optional: Uses AWS credentials chain if not specified
    # aws_access_key_id: ${AWS_ACCESS_KEY_ID}
    # aws_secret_access_key: ${AWS_SECRET_ACCESS_KEY}

  temperature: 0.2
  max_tokens: 4000
  cache:
    enabled: true
```

**Option 3: Anthropic Direct (Planned - Phase 3.4)**
```yaml
llm:
  enabled: true
  provider: anthropic

  anthropic:
    api_key: ${ANTHROPIC_API_KEY}
    model: claude-3-5-sonnet-20241022

  temperature: 0.2
  max_tokens: 4000
  requests_per_minute: 50  # Anthropic tier limits
  cache:
    enabled: true
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
git clone https://github.com/slb350/drep.git
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

See **[docs/roadmap.md](docs/roadmap.md)** for the complete development roadmap with priorities, timelines, and contribution opportunities.

### Current Status (v0.1.0+)
- ✅ Gitea adapter with full PR review support
- ✅ GitHub adapter with full CLI integration
- ✅ LLM-powered code quality analysis (Python)
- ✅ Intelligent caching (80%+ hit rate)
- ✅ Circuit breaker & rate limiting
- ✅ Docstring generator for Python
- ✅ CLI interface with metrics tracking

### Development Progress (5 Development Phases)

**🎯 Phase 1: Quick Wins** (Sprint 1-2) ✅ COMPLETE
- Security audit, BaseAdapter interface, extract constants
- 22 new tests added, 390 total tests passing

**🔧 Phase 2: Quality & Testing** (Sprint 3-4) ✅ COMPLETE
- E2E integration tests, API documentation, dependency injection
- 18 new tests added, 411 total tests passing

**🚀 Phase 3: Platform & LLM Backend Expansion** (Sprint 5-8) - IN PROGRESS
- ✅ Phase 3.1: GitHub adapter (API complete, 58 unit + 6 integration tests)
- ✅ Phase 3.2: CLI integration for GitHub (scan & review commands)
- 🔜 Phase 3.3: AWS Bedrock LLM provider (4-6 hours, enterprise compliance)
- 🔜 Phase 3.4: Anthropic Direct LLM provider (3-4 hours, latest Claude models)
- 🔜 Phase 3.5: GitLab adapter support

**🌟 Phase 4: Feature Expansion** (Sprint 9-12)
- Multi-language support (JavaScript, TypeScript, Go, Rust)
- Web UI dashboard for viewing findings and metrics

**🔬 Phase 5: Advanced Features** (Backlog)
- Custom rules engine, performance optimizations, vector database for cross-file context

**Want to help?** Good first issues: GitHub adapter implementation, GitLab adapter implementation, adding benchmarks. See [docs/roadmap.md](docs/roadmap.md#-contributing) for details.

## Comparison with Existing Tools

| Feature | drep (current) | Greptile | PR-Agent | Codedog |
|---------|----------------|----------|----------|---------|
| **CLI repository scans** | ✅ | ❌ | ❌ | ❌ |
| **Docstring suggestions (Python)** | ✅ | ❌ | ❌ | ❌ |
| **Gitea PR reviews** | ✅ | ❌ | ❌ | ❌ |
| **Local LLM** | ✅ | ❌ | Partial | Partial |
| **Gitea support** | ✅ Full | ❌ | ❌ | ❌ |
| **GitHub support** | ✅ Full | ✅ | ✅ | ✅ |
| **GitLab support** | 🚧 Planned | ✅ | ✅ | ✅ |
| **Draft PR automation** | 🚧 Planned | ❌ | ❌ | ❌ |

**Key Differentiator**: drep focuses on local, privacy-preserving analysis with docstring intelligence and PR reviews powered by your own LLM. GitLab support is planned for the next phase.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Support

- **Documentation**: [docs/](docs/)
- **Issues**: https://github.com/slb350/drep/issues
- **Discussions**: https://github.com/slb350/drep/discussions

## Acknowledgments

- Uses OpenAI-compatible local LLMs (LM Studio, Ollama)
- Inspired by tools like Greptile, PR-Agent, and Codedog
- Thanks to the open-source community

---

**Made with ❤️ for developers who care about code quality and documentation**
