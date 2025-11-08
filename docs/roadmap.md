# Drep Development Roadmap

**Last Updated:** 2025-11-07
**Current Version:** v0.1.0

This roadmap outlines planned improvements for drep, organized by priority and effort. Items are sequenced from quick wins (easy, high-impact) to complex long-term projects.

---

## 🎯 Phase 1: Quick Wins (Sprint 1-2) ✅ COMPLETE

**Completed:** 2025-11-07 | **Branch:** feature/phase1 | **PR:** #TBD

All Phase 1 quick wins completed with TDD methodology. 22 new tests added, 390 total tests passing.

### 1.1 Security Audit ⚠️ CRITICAL
**Effort:** Small | **Impact:** Critical | **Status:** ✅ Complete (2025-11-07)

Audit all logging statements to ensure API keys and tokens are never logged.

**Tasks:**
- [x] Search codebase for all `logger.info/debug/warning` statements (94 reviewed)
- [x] Verify no variables named `token`, `key`, `password`, `secret` are logged (✓ Clean)
- [x] Add secret detection utilities (drep/security/)
- [x] Document safe logging practices (docs/SECURITY.md)
- [x] Fix 2 critical token exposure bugs in HTTP error logging

**Deliverables:**
- `drep/security/detector.py` - Secret detection and URL sanitization
- `docs/SECURITY.md` - Comprehensive safe logging guidelines
- 8 new tests, all passing
- **Commit:** fec29b2

---

### 1.2 Complete BaseAdapter Abstract Class
**Effort:** Small | **Impact:** High | **Status:** ✅ Complete (2025-11-07)

Enforce interface contracts for platform adapters to ensure consistency.

**Tasks:**
- [x] Define abstract methods in `drep/adapters/base.py` (7 methods)
- [x] Add type hints for all parameters and return values
- [x] Document expected exceptions
- [x] Update GiteaAdapter to inherit from BaseAdapter
- [x] Add post_review_comment() and get_file_content() methods
- [x] Add tests for base interface

**Deliverables:**
- `drep/adapters/base.py` - Abstract base class with 7 required methods
- `drep/adapters/gitea.py` - Updated to inherit and implement all methods
- 6 new tests, all passing
- **Commit:** 6b8917e

---

### 1.3 Extract Configuration Constants
**Effort:** Small | **Impact:** Low | **Status:** ✅ Complete (2025-11-07)

Replace magic numbers with named constants for better readability.

**Tasks:**
- [x] Create `drep/constants.py` for shared constants
- [x] Extract and document magic numbers (3 constants)
- [x] Add comprehensive docstrings explaining rationale
- [x] Update all references in llm/client.py and llm/cache.py
- [x] Add tests verifying constants are used

**Deliverables:**
- `drep/constants.py` - 3 constants with "why this value" documentation
- 8 new tests, all passing
- **Commit:** bfc5be8

---

### 1.4 Enhanced Markdown Linting
**Effort:** Small | **Impact:** Medium | **Status:** ✅ Complete (2025-11-07)

Integrate markdownlint for comprehensive documentation quality checks.

**Tasks:**
- [x] Create `.markdownlint.json` configuration
- [x] Add `drep lint-docs` CLI command (text and JSON output)
- [x] Use existing DocumentationAnalyzer (10 comprehensive checks)
- [x] Pure Python solution (no Node.js dependency)

**Deliverables:**
- `.markdownlint.json` - Project-specific markdown rules
- `drep lint-docs` - CLI command for on-demand linting
- DocumentationAnalyzer with 10 checks (already implemented)
- **Commit:** 743dfc0

**Configuration example:**
```json
{
  "default": true,
  "MD013": false,
  "MD033": {
    "allowed_elements": ["img", "br", "details", "summary"]
  },
  "MD041": false
}
```

**Benefits:**
- Consistent documentation style across project
- Catches formatting issues (headings, lists, code blocks)
- Improves readability and professional appearance
- Complements existing 10 basic markdown checks

**Integration:**
- Extends `drep/documentation/markdown_analyzer.py`
- Results appear in same findings list as other checks
- Optional via `documentation.markdown_lint: true` config

---

## 🔧 Phase 2: Quality & Testing (Sprint 3-4) ✅ COMPLETE

**Completed:** 2025-11-07 | **Branch:** feature/phase2 | **PR:** #TBD

All Phase 2 items completed using strict TDD methodology. 18 new tests added, 411 total tests passing.

### 2.1 Add End-to-End Integration Tests
**Effort:** Medium | **Impact:** High | **Status:** ✅ Complete (2025-11-07)

Integration tests for LLM client workflows with dependency injection.

**Tasks:**
- [x] Create `tests/integration/test_end_to_end_workflows.py`
- [x] Test scenarios (6 tests):
  - Dependency injection workflow
  - Caching workflow (cold/warm requests)
  - Rate limiting workflow
  - Circuit breaker workflow
  - Metrics tracking workflow
  - Backward compatibility workflow
- [x] Use test fixtures (temp_cache_dir, mock_http_response)
- [x] Mock LLM responses for consistency
- [x] Proper mocking of open-agent-sdk and HTTP layers

**Deliverables:**
- 6 new integration tests, all passing
- Tests verify Items 2.2 and 2.4 work end-to-end
- **Commit:** 6a3d4a0

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
**Effort:** Small | **Impact:** Medium | **Status:** ✅ Complete (2025-11-07)

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
- [x] Add deprecation warnings to legacy metric properties
- [x] Convert to private attributes (_total_requests, _total_tokens, _failed_requests)
- [x] Add @property wrappers with DeprecationWarning
- [x] Update all internal call sites to use private attributes
- [x] Add tests verifying deprecation warnings

**Deliverables:**
- 5 new deprecation tests, all passing
- Backward compatibility maintained (properties still work)
- **Commits:** 1756236, 05f220a

---

### 2.3 Generate API Documentation
**Effort:** Small | **Impact:** Medium | **Status:** ✅ Complete (2025-11-07)

Create professional API documentation using Sphinx.

**Tasks:**
- [x] Install Sphinx 8.2.3 and sphinx-rtd-theme 3.0.2
- [x] Initialize Sphinx in `docs/api/source/`
- [x] Configure autodoc extension
- [x] Create modules.rst with comprehensive API coverage
- [x] Update index.rst with project intro and quick start
- [x] Configure RTD theme

**Deliverables:**
- API documentation structure in `docs/api/`
- Covers: LLM Client, Circuit Breaker, Metrics, Cache, Analyzers, Scanner
- **Commit:** 5b79f35

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
**Effort:** Medium | **Impact:** Medium | **Status:** ✅ Complete (2025-11-07)

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

**Tasks:**
- [x] Add rate_limiter parameter to LLMClient.__init__
- [x] Add circuit_breaker parameter with sentinel value
- [x] Use injected dependencies or create defaults
- [x] Maintain backward compatibility
- [x] Add 7 dependency injection tests

**Deliverables:**
- 7 new dependency injection tests, all passing
- Full backward compatibility (defaults created if not injected)
- **Commits:** 5846967, 0fb1849

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
