# Drep Development Roadmap

**Last Updated:** 2025-11-07
**Current Version:** v0.1.0

This roadmap outlines planned improvements for drep, organized by priority and effort. Items are sequenced from quick wins (easy, high-impact) to complex long-term projects.

---

## 🎯 Phase 1: Quick Wins (Sprint 1-2)

These are small-effort, high-impact improvements that can be completed quickly.

### 1.1 Security Audit ⚠️ CRITICAL
**Effort:** Small | **Impact:** Critical | **Status:** Not Started

Audit all logging statements to ensure API keys and tokens are never logged.

**Tasks:**
- [ ] Search codebase for all `logger.info/debug/warning` statements
- [ ] Verify no variables named `token`, `key`, `password`, `secret` are logged
- [ ] Add pre-commit hook to prevent secret logging
- [ ] Document safe logging practices in CONTRIBUTING.md

**Files to review:**
- `drep/llm/client.py`
- `drep/adapters/*.py`
- `drep/core/*.py`

---

### 1.2 Complete BaseAdapter Abstract Class
**Effort:** Small | **Impact:** High | **Status:** Not Started

Enforce interface contracts for platform adapters to ensure consistency.

**Tasks:**
- [ ] Define abstract methods in `drep/adapters/base.py`:
  - `create_issue()`
  - `get_pull_request()`
  - `post_review_comment()`
  - `get_file_content()`
- [ ] Add type hints for all parameters and return values
- [ ] Document expected exceptions
- [ ] Update GiteaAdapter to inherit from BaseAdapter
- [ ] Add tests for base interface

**Benefits:**
- Easier to add new platform adapters (GitHub, GitLab)
- Compile-time checks for interface compliance
- Better IDE autocomplete support

**Reference:** See CODEBASE_ANALYSIS.md Section 1 for implementation example.

---

### 1.3 Extract Configuration Constants
**Effort:** Small | **Impact:** Low | **Status:** Not Started

Replace magic numbers with named constants for better readability.

**Tasks:**
- [ ] Create `drep/constants.py` for shared constants
- [ ] Extract and document magic numbers:
  - `MAX_ESTIMATED_TOKENS = 50000` (llm/client.py)
  - `TEMPERATURE_TOLERANCE = 0.01` (llm/cache.py)
  - `REPO_SEMAPHORE_TTL_SECONDS = 600` (llm/rate_limiter.py)
- [ ] Add explanatory comments for each constant
- [ ] Update all references to use constants

**Before:**
```python
estimated_tokens = max(1, min(estimated_tokens, 50000))  # Why 50000?
```

**After:**
```python
# Maximum estimated tokens to reserve (prevents over-reservation)
MAX_ESTIMATED_TOKENS = 50000
estimated_tokens = max(1, min(estimated_tokens, MAX_ESTIMATED_TOKENS))
```

---

## 🔧 Phase 2: Quality & Testing (Sprint 3-4)

Medium-effort improvements to testing and code quality.

### 2.1 Add End-to-End Integration Tests
**Effort:** Medium | **Impact:** High | **Status:** Not Started

Test complete workflows from git clone to issue creation.

**Tasks:**
- [ ] Create `tests/integration/test_full_workflow.py`
- [ ] Test scenarios:
  - Full repository scan (cold scan)
  - Incremental scan (with cache hits)
  - PR review workflow
  - Issue creation with Gitea API
  - Error recovery (LLM failures, network errors)
- [ ] Use test fixtures for reproducible repos
- [ ] Mock LLM responses for consistency
- [ ] Add to CI/CD pipeline

**Example test:**
```python
@pytest.mark.integration
async def test_full_scan_workflow():
    """Test complete workflow: clone → analyze → create issues."""
    # Setup test repo with known bugs
    test_repo = create_test_repo_with_bugs()

    # Run scanner
    scanner = RepositoryScanner(config)
    findings = await scanner.scan_repository(test_repo.url)

    # Verify findings
    assert len(findings) > 0
    assert any(f.category == "bug" for f in findings)

    # Verify issues created on Gitea
    issues = await gitea_client.get_issues(test_repo)
    assert len(issues) == len(findings)
```

---

### 2.2 Deprecate Legacy Metrics
**Effort:** Small | **Impact:** Medium | **Status:** Not Started

Remove duplicate metrics tracking in LLMClient.

**Current issue:**
```python
# Legacy metrics (duplicated)
self.total_requests = 0
self.total_tokens = 0

# New metrics (preferred)
self.metrics = LLMMetrics()
```

**Tasks:**
- [ ] Add deprecation warnings to legacy metric properties
- [ ] Update all call sites to use `metrics` object
- [ ] Remove legacy metrics in next major version
- [ ] Update documentation

---

### 2.3 Generate API Documentation
**Effort:** Small | **Impact:** Medium | **Status:** Not Started

Create professional API documentation using Sphinx.

**Tasks:**
- [ ] Install Sphinx and sphinx-rtd-theme
- [ ] Initialize Sphinx in `docs/api/`
- [ ] Configure autodoc extension
- [ ] Generate API docs from docstrings
- [ ] Add architecture diagrams
- [ ] Set up Read the Docs hosting (optional)
- [ ] Add link to README.md

**Commands:**
```bash
pip install sphinx sphinx-rtd-theme
cd docs/api/
sphinx-quickstart
sphinx-apidoc -o source/ ../../drep/
make html
```

---

### 2.4 Dependency Injection for LLMClient
**Effort:** Medium | **Impact:** Medium | **Status:** Not Started

Improve testability by injecting dependencies instead of creating them.

**Current:**
```python
class LLMClient:
    def __init__(self, endpoint, model, ...):
        self.rate_limiter = RateLimiter(...)  # Hard to mock
        self.circuit_breaker = CircuitBreaker(...)
```

**Proposed:**
```python
class LLMClient:
    def __init__(
        self,
        endpoint: str,
        model: str,
        rate_limiter: Optional[RateLimiter] = None,
        circuit_breaker: Optional[CircuitBreaker] = None,
        ...
    ):
        self.rate_limiter = rate_limiter or RateLimiter(...)
        self.circuit_breaker = circuit_breaker or CircuitBreaker(...)
```

**Benefits:**
- Easier to test (inject mocks)
- More flexible for advanced users
- Better separation of concerns

---

## 🚀 Phase 3: Platform Expansion (Sprint 5-8)

Large projects to add GitHub and GitLab support.

### 3.1 Complete GitHub Adapter
**Effort:** Large | **Impact:** High | **Status:** Not Started

Full GitHub API integration.

**Tasks:**
- [ ] Implement GitHubAdapter in `drep/adapters/github.py`
- [ ] Use PyGithub or GitHub REST API v3
- [ ] Implement all BaseAdapter methods:
  - `create_issue()` - Use GitHub Issues API
  - `get_pull_request()` - Use Pull Requests API
  - `post_review_comment()` - Use Review Comments API
  - `get_file_content()` - Use Contents API
- [ ] Add GitHub authentication (personal access token)
- [ ] Handle GitHub API rate limiting
- [ ] Add configuration in `config.yaml`
- [ ] Write integration tests
- [ ] Update documentation

**API endpoints:**
```python
# GitHub REST API v3
POST /repos/{owner}/{repo}/issues
GET /repos/{owner}/{repo}/pulls/{number}
POST /repos/{owner}/{repo}/pulls/{number}/comments
GET /repos/{owner}/{repo}/contents/{path}
```

---

### 3.2 Complete GitLab Adapter
**Effort:** Large | **Impact:** High | **Status:** Not Started

Full GitLab API integration.

**Tasks:**
- [ ] Implement GitLabAdapter in `drep/adapters/gitlab.py`
- [ ] Use python-gitlab or GitLab REST API v4
- [ ] Implement all BaseAdapter methods
- [ ] Add GitLab authentication (personal access token)
- [ ] Handle GitLab API rate limiting
- [ ] Add configuration in `config.yaml`
- [ ] Write integration tests
- [ ] Update documentation

---

## 🌟 Phase 4: Feature Expansion (Sprint 9-12)

Major feature additions for broader applicability.

### 4.1 Multi-Language Support
**Effort:** Large | **Impact:** High | **Status:** Not Started

Support JavaScript, TypeScript, Go, Rust beyond Python.

**Tasks:**
- [ ] Create language-specific analyzers:
  - `drep/analyzers/javascript.py` - ESLint integration
  - `drep/analyzers/typescript.py` - TSLint/TypeScript compiler
  - `drep/analyzers/go.py` - go vet, staticcheck
  - `drep/analyzers/rust.py` - clippy
- [ ] Add language detection by file extension
- [ ] Create language-specific prompts
- [ ] Add tests for each language
- [ ] Update documentation

**Languages priority:**
1. JavaScript/TypeScript (high demand)
2. Go (common for backend)
3. Rust (growing adoption)
4. Java (enterprise)
5. C/C++ (systems programming)

---

### 4.2 Web UI Dashboard
**Effort:** Large | **Impact:** Medium | **Status:** Not Started

Web interface for viewing findings and metrics.

**Features:**
- Interactive dashboard with charts
- Browse scan history
- View findings by severity/category
- Metrics over time (cost, hit rate)
- Configure settings via UI
- Team collaboration features

**Tech stack:**
- Backend: Expand FastAPI server
- Frontend: React or Vue.js
- Database: Expand SQLite schema
- Charts: Chart.js or D3.js

**Tasks:**
- [ ] Design UI mockups
- [ ] Implement REST API endpoints
- [ ] Build frontend SPA
- [ ] Add authentication (optional)
- [ ] Deploy as Docker container
- [ ] Write user documentation

---

## 🔬 Phase 5: Advanced Features (Backlog)

Nice-to-have features and optimizations.

### 5.1 Refactor Long Functions
**Effort:** Medium | **Impact:** Low | **Status:** Not Started

Break down functions >100 lines into smaller methods.

**Target functions:**
- `drep/llm/client.py` - `analyze_code()` (~100 lines)
- `drep/llm/client.py` - `analyze_code_json()` (~80 lines)

**Strategy:**
- Extract helper methods for distinct phases
- Each method should have single responsibility
- Improve testability by testing individual phases

---

### 5.2 Performance Benchmarks
**Effort:** Medium | **Impact:** Low | **Status:** Not Started

Track performance over time to detect regressions.

**Tasks:**
- [ ] Add benchmark tests with pytest-benchmark
- [ ] Benchmark cache performance (lookups, writes)
- [ ] Benchmark LLM request latency
- [ ] Benchmark file parsing (AST, regex)
- [ ] Set performance targets (SLAs)
- [ ] Add benchmarks to CI/CD

---

### 5.3 Batch API Requests
**Effort:** Medium | **Impact:** Medium | **Status:** Not Started

Combine multiple small files into single LLM request.

**Benefits:**
- Reduced API calls (fewer HTTP round-trips)
- Lower costs (amortized per-request overhead)
- Faster for repos with many small files

**Trade-offs:**
- More complex response parsing
- Risk of hitting context window limits
- Harder to handle per-file errors

**Implementation:**
```python
# Instead of:
for file in small_files:
    result = await llm.analyze_code(file)

# Consider:
batch = small_files[:10]  # Batch up to 10 small files
combined_code = "\n\n--- FILE: " + "\n\n--- FILE: ".join(batch)
result = await llm.analyze_code(combined_code)
# Parse result and split findings by file
```

---

### 5.4 Property-Based Testing
**Effort:** Medium | **Impact:** Low | **Status:** Not Started

Use Hypothesis for testing edge cases.

**Example:**
```python
from hypothesis import given, strategies as st

@given(
    code=st.text(min_size=1, max_size=10000),
    model=st.sampled_from(["gpt-4", "llama-2-70b"]),
    temperature=st.floats(min_value=0.0, max_value=2.0),
)
async def test_analyze_code_handles_all_inputs(code, model, temperature):
    """Test analyzer handles arbitrary valid inputs without crashing."""
    client = LLMClient(endpoint="http://test", model=model, temperature=temperature)
    try:
        await client.analyze_code("test prompt", code)
    except ValueError:  # Expected for invalid code
        pass
```

---

### 5.5 Custom Rules Engine
**Effort:** Medium | **Impact:** Medium | **Status:** Not Started

Allow users to define custom linting rules.

**Example config:**
```yaml
custom_rules:
  - name: "No print statements"
    pattern: "\\bprint\\("
    severity: "low"
    message: "Use logging instead of print()"

  - name: "Require error handling"
    llm_prompt: "Check if function handles errors properly"
    trigger: "def.*:$"
    severity: "medium"
```

**Benefits:**
- Organization-specific best practices
- Domain-specific rules
- Complements LLM analysis

---

## 📊 Success Metrics

Track these metrics to measure roadmap progress:

### Code Quality
- [ ] Test coverage: Target 90%+ (current: ~85%)
- [ ] Zero critical security issues
- [ ] All functions <50 lines (refactoring goal)
- [ ] Consistent naming conventions

### Performance
- [ ] Cache hit rate: 80%+ (current: ✅)
- [ ] Average scan time: <5 minutes for 1000-file repo
- [ ] LLM cost per scan: <$5 with caching

### Feature Completeness
- [ ] 3+ platform adapters (Gitea ✅, GitHub, GitLab)
- [ ] 3+ language support (Python ✅, JavaScript, Go)
- [ ] Web UI dashboard available

### Adoption
- [ ] 100+ GitHub stars
- [ ] 10+ external contributors
- [ ] 1000+ PyPI downloads/month

---

## 🗓️ Timeline

| Phase | Duration | Timeline | Deliverables |
|-------|----------|----------|--------------|
| Phase 1: Quick Wins | 2 sprints | Sprint 1-2 | Security audit, BaseAdapter, constants |
| Phase 2: Quality & Testing | 2 sprints | Sprint 3-4 | E2E tests, API docs, DI refactor |
| Phase 3: Platform Expansion | 4 sprints | Sprint 5-8 | GitHub adapter, GitLab adapter |
| Phase 4: Feature Expansion | 4 sprints | Sprint 9-12 | Multi-language, Web UI |
| Phase 5: Advanced Features | Ongoing | Backlog | Performance, optimization |

**Sprint length:** 2 weeks

---

## 🤝 Contributing

Want to help with the roadmap? See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

**Good first issues:**
- Extract configuration constants (Phase 1.3)
- Add performance benchmarks (Phase 5.2)
- Write additional E2E tests (Phase 2.1)

**Looking for:**
- Frontend developers for Web UI (Phase 4.2)
- Language experts for multi-language support (Phase 4.1)
- Platform experts for GitHub/GitLab adapters (Phase 3)

---

## 📚 References

- [Technical Design](./technical-design.md) - Architecture details
- [LLM Setup Guide](./llm-setup.md) - LLM configuration
- [CHANGELOG.md](../CHANGELOG.md) - Release history
- [GitHub Issues](https://github.com/slb350/drep/issues) - Bug reports and feature requests

---

**Note:** This roadmap is a living document and will be updated as priorities change and features are completed. Last comprehensive review from CODEBASE_ANALYSIS.md on 2025-11-07.
