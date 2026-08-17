# Multi-Language Support Analysis

**Created:** 2025-11-09
**Last Validated:** 2025-11-09 (Codex deep dive)
**Status:** Partly delivered in 1.3.0 — see the note below
**Goal:** Determine whether multi-language support can be achieved via additive analyzers or requires foundational refactoring

---

## 0. Status update (1.3.0)

The LLM-review half of this plan shipped without the tree-sitter foundation it assumed.
The reason: **the LLM parses nothing.** Feeding TypeScript to the analyzer through the
then-unmodified Python prompt produced five findings, all TypeScript-correct, including
two that only make sense in TypeScript. So code review for JavaScript, TypeScript, Go and
Rust needed a registry, a prompt per language, and a wider file filter — hours, not the
15-20 estimated for Phase 1.

What shipped in 1.3.0:

- `drep/languages/` — `LanguageSupport` + registry (§5.1, §5.2), driving discovery,
  analyzer support, prompts and cache keys (§5.3)
- Deterministic tool integration (§4.1 "External lint hook") as the *primary* gating
  layer rather than an optional extra — ruff, eslint, tsc, gofmt, go vet, clippy
- Language-aware code-quality and PR-review prompts (§5.4)

What this document still describes accurately, and is **not** done:

- `extract_symbols` / tree-sitter (§5.1). Still required, but only for **doc-comment
  generation** in non-Python languages — the one feature that genuinely needs an AST.
- The `languages:` config block (§5.5). Every registered language is currently active.

The effort table in §6 should be read as applying to doc-comment generation only.

---

## 1. Purpose & Scope

This document validates the current Python-first implementation, captures the gaps that block additional languages, and outlines a detailed plan for delivering JavaScript/TypeScript, Go, and Rust coverage without regressing existing Python behavior. The audience is engineering leadership plus the contributors who will own the Phase 4.2 workstream.

---

## 2. Validation Snapshot (2025-11-09)

| Area | Source of Truth | Observation | Impact |
| --- | --- | --- | --- |
| Repository scanning | `drep/core/scanner.py:103-318` | All discovery, diff, and staged-file logic hard-code `.py`/`.md` filters; analyzer routing assumes a single Python analyzer | Non-Python files are never queued; incremental scans will silently skip new languages |
| Code quality analyzer | `drep/code_quality/analyzer.py:10-118` | One global `PYTHON_ANALYSIS_PROMPT`; `is_supported_file` enforces `.py` only | No extension point for prompts, severity tuning, or analyzer selection per language |
| Docstring pipeline | `drep/docstring/ast_utils.py:1-200`, `drep/docstring/generator.py:1-160` | Direct dependency on the stdlib `ast` module and Google-style docstrings | Cannot reuse AST extraction for other languages; heuristics like `FunctionInfo` are Python-specific |
| PR review analyzer | `drep/pr_review/analyzer.py:1-155` | Prompt explicitly says “You are a senior Python engineer”; rubric references PEP8/type hints | PR reviews for other languages would immediately contain incorrect guidance |
| Config & CLI | `drep/models/config.py`, `drep/cli/check.py` (not shown) | No schema/flags for per-language enablement, lint tooling, or resource limits | Users cannot opt-in/out or provide language-specific settings |

**Conclusion:** Multi-language enablement is not an additive analyzer; it demands architectural refactoring plus new configuration, prompt strategy, and test coverage.

---

## 3. Verified Python-Only Coupling

### 3.1 Repository Scanner
- `get_scan_targets` only walks for `.py` and `.md` (`drep/core/scanner.py`, policy in `drep/core/file_targets.py`).
- `_get_changed_files` and `get_staged_files` repeat `.py`/`.md` suffix checks (`drep/core/scanner.py:230-318`), which duplicates logic and makes new extensions hard to add consistently.
- `analyze_code_quality` filters to `self.code_analyzer.is_supported_file`, which currently returns True only for `.py` (`drep/core/scanner.py:320-420` + `drep/code_quality/analyzer.py:107-118`).

### 3.2 Code Quality Analyzer
- Prompt content references PEP8, Python naming, and docstrings (`drep/code_quality/analyzer.py:17-64`).
- The analyzer always reports issues under the `"code_quality"` analyzer key, making it impossible to differentiate caching/metrics by language (`drep/code_quality/analyzer.py:79-105`).

### 3.3 Docstring Generator + AST Utilities
- `extract_functions` and `extract_classes` rely on `ast.parse`, `ast.FunctionDef`, Python-only decorator syntax, and positional-only args (`drep/docstring/ast_utils.py:45-200`).
- The docstring prompt instructs the LLM to output Google-style Python docstrings (`drep/docstring/generator.py:32-103`), which would be invalid for JS (JSDoc), Go (line comments), or Rust (`///`).

### 3.4 PR Review Analyzer
- The PR prompt states “You are a senior Python engineer” and lists Python-only quality gates (PEP8, type hints) (`drep/pr_review/analyzer.py:15-93`).
- Severity vocabulary (`info|suggestion|warning|critical`) is reasonable across languages but the guidance and examples need language context.

### 3.5 Configuration & Telemetry
- `Config` lacks a `languages` section, so there is no place to express defaults, per-language lint tooling, or enablement toggles.
- Metrics, cache keys, and progress tracking all log “Python” implicitly via analyzer names; adding languages without unique identifiers would make observability ambiguous.

---

## 4. Product Requirements for Multi-Language Support

### 4.1 Functional Scope

| Capability | Python (current) | JavaScript / TypeScript | Go | Rust |
| --- | --- | --- | --- | --- |
| File discovery | ✅ `.py`, `.md` | ❌ | ❌ | ❌ |
| Code quality LLM review | ✅ (prompt + findings) | ⏳ (needs JS/TS prompt + AST extraction) | ⏳ | ⏳ |
| Docstring / comment quality | ✅ Google-style docstrings | ⏳ JSDoc/JSDoc-lite | ⏳ standard Go doc comments | ⏳ `///` + `//!` doc comments |
| PR review feedback | ✅ (Python rubric) | ⏳ (JS/TS rubric) | ⏳ | ⏳ |
| External lint hook | ⚙️ None (pure LLM) | Optional ESLint/TS compiler diagnostics | Optional `golangci-lint` | Optional `clippy` |
| Configuration | Global, language-agnostic | Needs per-language toggles + tooling paths | Same | Same |

### 4.2 Non-Functional Requirements
1. **Backward compatibility:** Python users must see identical behavior unless they opt into additional languages.
2. **Extensibility:** Adding a new language must be limited to registering a `LanguageSupport` implementation plus prompts/config entries.
3. **Performance:** Avoid re-scanning files unnecessarily; language detection must be O(1) by extension.
4. **Caching:** LLM cache keys need to include the language/analyzer name to prevent cross-language leakage.
5. **Observability:** Metrics (e.g., analyzer latency, findings per file) must include `language` tags for dashboards.
6. **Security/Isolation:** Optional subprocess-based linters (Go/Rust) must be sandboxed and respect repo-local execution policies.

---

## 5. Target Architecture

### 5.1 Language Support Layer

```
drep/languages/
├── __init__.py
├── base.py              # Abstract protocol
├── registry.py          # Extension discovery
├── python.py            # Existing behavior
├── javascript.py        # JS/TS
├── go.py
└── rust.py
```

`base.py` exposes the minimal surface area needed by scanners, analyzers, and docstring generators:

```python
class LanguageSupport(ABC):
    name: str
    file_extensions: list[str]

    def get_analysis_prompt(self) -> str: ...
    def get_doc_prompt(self) -> str: ...
    def normalize_finding(self, finding: Finding) -> Finding: ...
    def extract_symbols(self, code: str) -> SymbolGraph: ...
    def doc_style(self) -> DocStyle: ...
    def auxiliary_linters(self) -> list[LinterConfig]: ...
```

Implementation details:
- `extract_symbols` can internally use tree-sitter, native AST tooling, or regexes.
- `DocStyle` encapsulates formatting requirements (e.g., Google docstring vs JSDoc vs `///`).
- `auxiliary_linters` provides command templates plus severity remapping for optional static analyzers.

### 5.2 Language Registry

- Maintains `{extension -> language}` and `{language -> LanguageSupport}` maps.
- Provides helpers used throughout the codebase:
  - `detect_language(path: str) -> Optional[LanguageSupport]`
  - `supported_extensions(include_docs: bool = True) -> list[str]`
  - `iter_languages(enabled_only=True)`
- Accepts dependency injection so that tests can register fake languages without touching global singletons.

### 5.3 Pipeline Integration

1. **Scanner (`drep/core/scanner.py`):**
   - `get_scan_targets` → registry-driven suffix set in `drep/core/file_targets.py`.
   - `_get_changed_files` / `get_staged_files` filter using `registry.supported_extensions`.
   - `scan_repository` records the enabled language list in scan metadata for auditability.

2. **Code Quality Analyzer (`drep/code_quality/analyzer.py`):**
   - Accept `LanguageRegistry` and look up the `LanguageSupport` per file.
   - Analyzer name should include the language (e.g., `code_quality_python`) to preserve metrics/cache segmentation.
   - Prompts move into each `LanguageSupport` implementation; shared scaffolding remains in the analyzer.

3. **Docstring / Comment Analyzer:**
   - Replace direct `extract_functions` call with `language.extract_symbols`.
   - Route style-specific prompts (Google docstrings, JSDoc, Rust doc comments) via `language.get_doc_prompt`.
   - Keep `DocstringGenerator` as orchestrator; rename to `DocumentationGenerator` once multiple languages exist.

4. **PR Review Analyzer:**
   - Parameterize prompts by dominant language(s) in the diff; fallback to per-file language for inline comments.
   - Validate added-line coordinates the same way, but include `language` in each comment payload for downstream consumers.

5. **LLM Cache & Metrics:**
   - Cache key format: `{analyzer}:{language}:{repo_id}:{commit_sha}:{file_path}`.
   - Metrics tags: `language`, `analyzer`, `provider`.

### 5.4 Prompt Strategy

- **Python:** Existing prompt becomes `language.get_analysis_prompt()`.
- **JavaScript/TypeScript:** Highlight async/await pitfalls, TypeScript `any`, React hooks, Node vs browser context.
- **Go:** Emphasize error handling, goroutine leaks, `defer` usage, concurrency race patterns.
- **Rust:** Focus on ownership/borrowing, unsafe blocks, trait bounds, `Send/Sync` correctness.
- Prompts must share core JSON schemas to keep parsing logic unchanged.

### 5.5 Configuration Schema

Add to `config.yaml` / `Config` model:

```yaml
languages:
  enabled: ["python", "javascript", "typescript", "go", "rust"]
  defaults:
    python:
      docstring_style: google
      lint: null
    javascript:
      docstring_style: jsdoc
      lint:
        command: ["npx", "eslint", "--format", "json"]
        severity_map:
          error: high
          warning: medium
    go:
      lint:
        command: ["golangci-lint", "run", "--out-format", "json"]
    rust:
      lint:
        command: ["cargo", "clippy", "--message-format", "json"]
```

Validation rules:
- Unknown languages in `enabled` raise a config error.
- Linter commands are optional; when omitted we remain LLM-only.

### 5.6 Data & Persistence

- No schema changes required for the SQLite cache, but we must persist the `language` string on each `Finding` to keep UI/API parity.
- Repository scan table should remain unchanged; the new metadata can live in structured logs until we need reporting.

---

## 6. Implementation Plan & Effort

### Phase 0 – Research & Tooling Spike (4-6 hours)
- Evaluate `tree-sitter` vs language-specific parsers; prototype symbol extraction for JS & Go.
- Decide on packaging strategy (pre-built `tree-sitter` binaries vs runtime compile).
- Output: comparison matrix + go/no-go on tree-sitter investment.

### Phase 1 – Abstraction Foundation (v1.2.0, 15-20 hours)
1. Introduce `drep/languages` module, registry, and Python implementation.
2. Refactor scanner, analyzer, docstring generator, and PR review to be language-aware while still only enabling Python.
3. Update config schema with `languages.enabled` (default `["python"]`).
4. Add metrics/logging fields for `language`.
5. Tests:
   - Registry unit tests (extension detection, overrides).
   - Scanner tests across fake languages.
   - Regression tests ensuring Python-only behavior is unchanged when no other language is enabled.
6. Exit criteria: existing 395 tests still green; new unit tests cover registry and scanner fan-out.

### Phase 2 – JavaScript/TypeScript (v1.3.0, 15-20 hours)
1. Finalize parser strategy (tree-sitter or `esprima`/`typescript-eslint`).
2. Implement `JavaScriptSupport` with:
   - Extension set: `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`.
   - Language-specific analysis prompt & docstring prompt (JSDoc).
   - Symbol extraction for functions/classes/modules.
3. Optional ESLint integration gated by config.
4. Add 20+ tests covering detection, prompt selection, docstring formatting, and ESLint parsing.
5. Documentation updates (`README`, `docs/technical-design.md`, this file).
6. Exit criteria: `drep scan` surfaces JS/TS findings in sample repos; CLI flag `--languages js,ts` works.

### Phase 3 – Go (v1.4.0, 8-10 hours)
1. Implement `GoSupport` (extensions `.go`).
2. Go analysis prompt focuses on error handling, concurrency, `defer`, `context.Context`.
3. Optionally read `golangci-lint` JSON output and merge into findings.
4. Add doc comment heuristics (top-of-function `//` comments).
5. Add ~15 targeted tests with sample Go files.
6. Exit criteria: sample Go repo analyzed end-to-end; doc comment suggestions match Go style guide.

### Phase 4 – Rust (v1.5.0, 8-10 hours)
1. Implement `RustSupport` (extension `.rs`).
2. Prompt includes ownership, borrow checker pitfalls, and `unsafe` guidelines.
3. Optional `cargo clippy` ingestion.
4. Implement doc comment detection for `///` and module-level `//!`.
5. Add ~15 tests + integration run against a small Rust crate.
6. Exit criteria: LLM findings show Rust-specific messaging; doc comments respect `rustdoc` formatting.

**Cumulative Effort:** 46-60 hours, matching the earlier high-level estimate but now with explicit deliverables and gate checks.

---

## 7. Testing & Validation Strategy

1. **Unit Tests**
   - Registry: extension mapping, duplicate registration, case sensitivity.
   - Language adapters: prompt retrieval, docstyle formatting, auxiliary linter config.
   - Scanner: ensure only enabled language extensions are returned for mixed repos.
2. **Integration Tests**
   - Add sample fixtures per language in `tests/fixtures/<language>/repo`.
   - Simulate `drep scan` on each fixture and assert findings include correct `language`.
3. **Golden Tests for Prompts**
   - Store sanitized prompt snapshots for each language to catch accidental prompt regressions.
4. **Performance Regression Tests**
   - Benchmark scanning a repo with 1k mixed-language files; ensure the new registry adds <5% overhead.
5. **Manual Validation**
   - Run ESLint/golangci-lint/clippy integration manually on representative repos before enabling in CI.
6. **Telemetry**
   - Emit structured logs per analyzer invocation to confirm languages route correctly in staging before GA.

---

## 8. Tooling & Dependency Evaluation

| Option | Pros | Cons | Recommendation |
| --- | --- | --- | --- |
| **Tree-sitter** | Unified API, mature grammars for JS/TS/Go/Rust, fast incremental parsing | Requires compiling shared library; adds binary artifact management | ✅ Recommended if Phase 0 spike confirms build pipeline is acceptable |
| **Language-specific parsers** (esprima, go/ast, syn) | Native accuracy, ecosystem-aligned | Multiple runtimes (Node, Go, Rust), complex subprocess orchestration, slower | 🚫 Avoid for initial rollout |
| **Regex/heuristic parsing** | Zero dependencies | Fragile, misses edge cases, not future-proof | 🚫 Only for stopgap metrics |

**Binary distribution note:** If we ship tree-sitter, prebuild `build/languages.so` for macOS/Linux and fall back to runtime compilation when unavailable. Cache in `.drep/tree-sitter/` to avoid repeated builds.

---

## 9. Risk Register

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Tree-sitter compile failures on user machines | Medium | High (multi-language unusable) | Provide prebuilt binaries + optional pure-Python fallback (regex) with warning |
| LLM prompt quality varies by language | Medium | Medium | Collect real-world snippets per language; add prompt regression tests and human review loop |
| Auxiliary linters increase runtime | Medium | Medium | Make linters opt-in per config, parallelize linters + LLM work, expose timing metrics |
| Cache pollution across languages | Low | Medium | Include `language` in cache key + analyzer name before Phase 2 |
| PR review prompt drift | Medium | Medium | Parameterize prompt by detected languages and add tests verifying language token injection |
| Documentation mismatch | Low | Medium | Update README/docs simultaneously with feature flags; ship sample config blocks |

---

## 10. Open Questions & Decisions Needed

1. **Tree-sitter delivery** – Do we commit compiled grammars to the repo or build during install?
2. **ESLint/golangci-lint/clippy availability** – Should we auto-install (risky) or require users to pre-install and configure paths?
3. **Docstring generator scope** – Do we extend it to code comments only, or also general documentation (e.g., TS interface comments)?
4. **PR review default language** – When diffs include multiple languages, do we create per-language review batches or a single combined review with annotations?
5. **Feature flag rollout** – Should JS/TS ship behind `--experimental-languages` before becoming GA?

---

## 11. Success Metrics & KPIs

### 11.1 Adoption Metrics
- **Phase 1**: Zero regression in Python analysis (100% backward compatibility)
- **Phase 2**: 50+ JavaScript/TypeScript repositories analyzed in first month
- **Phase 3-4**: 20+ Go/Rust repositories each within first month

### 11.2 Quality Metrics
- **False positive rate**: <5% per language (measured via user feedback)
- **Language detection accuracy**: 100% (file extension based)
- **Prompt effectiveness**: 80%+ findings marked as "helpful" by users
- **Cross-language consistency**: Severity levels map correctly across all languages

### 11.3 Performance Metrics
- **Scan time overhead**: <10% increase with multi-language enabled
- **Cache hit rate**: >70% across all languages
- **Memory usage**: <2x increase with 4 languages enabled
- **Language detection latency**: <1ms per file

---

## 12. Migration Guide for Existing Users

### 12.1 Version Transition Path

**v1.1.0 → v1.2.0 (No action required)**
- Python-only behavior preserved by default
- Existing config.yaml remains valid
- Cache entries remain valid (language assumed to be Python)

**v1.2.0 → v1.3.0 (Opt-in to JavaScript)**
```yaml
# Add to config.yaml to enable JavaScript:
languages:
  enabled: ["python", "javascript", "typescript"]
```

**v1.3.0 → v1.4.0+ (Additional languages)**
```yaml
languages:
  enabled: ["python", "javascript", "typescript", "go", "rust"]
  # Per-language configuration optional
  defaults:
    go:
      lint:
        command: ["golangci-lint", "run", "--out-format", "json"]
```

### 12.2 Breaking Change Mitigation
- Old cache entries remain valid with implicit `language="python"`
- Existing API endpoints continue to work
- Metrics dashboards auto-updated with `language="python"` for historical data
- Finding format unchanged (only adds `language` field)

---

## 13. Cost Impact Analysis

### 13.1 LLM Token Usage
| Scenario | Tokens/File | Monthly Cost Impact |
| --- | --- | --- |
| Current (Python-only) | ~500 avg | Baseline |
| With language-specific prompts | ~600 avg | +20% |
| With auxiliary linter integration | ~550 avg | +10% |
| Multi-language repo (mixed) | ~580 avg | +16% |

### 13.2 Caching Efficiency
- Cache invalidation unchanged (commit SHA based)
- Language segmentation prevents false cache hits
- Net effect: Neutral to slight improvement in hit rate
- Storage: +5% due to language key in cache entries

### 13.3 Infrastructure Requirements
| Component | Impact | Mitigation |
| --- | --- | --- |
| Tree-sitter binary | +50MB disk | Pre-built binaries in release |
| Runtime memory | +100MB for grammars | Lazy loading per language |
| CI/CD pipeline | +5 minutes for tests | Parallel test execution |
| Network | Minimal (linter subprocesses) | Local execution only |

---

## 14. External Tooling Dependencies

### 14.1 Tool Requirements Matrix

| Language | Optional Tool | Min Version | Detection Method | Fallback | Priority |
|----------|--------------|-------------|------------------|----------|----------|
| Python | ruff/pylint | Any | `which ruff` | LLM-only | Optional |
| JavaScript | ESLint | ≥8.0 | `npm list eslint` | LLM-only | Recommended |
| TypeScript | typescript | ≥4.0 | `npm list typescript` | Treat as JS | Recommended |
| Go | golangci-lint | ≥1.50 | `golangci-lint version` | LLM-only | Optional |
| Rust | clippy | Stable | `cargo clippy --version` | LLM-only | Optional |

### 14.2 Auto-Detection Logic
```python
# Pseudocode for tool detection
def detect_linters():
    available = {}
    for lang, config in language_configs.items():
        if config.lint_command:
            try:
                result = subprocess.run([config.lint_command[0], "--version"],
                                      capture_output=True, timeout=1)
                if result.returncode == 0:
                    available[lang] = config.lint_command
            except (FileNotFoundError, TimeoutError):
                logger.info(f"Optional linter for {lang} not found, using LLM-only")
    return available
```

---

## 15. Documentation Impact

### 15.1 Required Documentation Updates

| Document | Changes Required | Priority |
| --- | --- | --- |
| README.md | Add "Supported Languages" section, multi-language examples | P0 |
| docs/technical-design.md | New "Language Architecture" section, updated component diagram | P0 |
| docs/llm-setup.md | Per-language prompt tuning, token usage by language | P1 |
| docs/roadmap.md | Update Phase 4.2 with implementation details | P1 |
| **NEW: docs/adding-languages.md** | Guide for contributors to add new languages | P1 |
| **NEW: docs/language-prompts.md** | Prompt engineering patterns per language | P2 |

### 15.2 API Documentation
- OpenAPI spec updates for language field in findings
- New endpoint: `GET /api/languages` (list enabled languages)
- Update webhook payloads to include language metadata

---

## 16. Example Implementation: JavaScript Support

### 16.1 Language Support Class

```python
# drep/languages/javascript.py
from typing import List, Optional
from drep.languages.base import LanguageSupport, LinterConfig, DocStyle

class JavaScriptSupport(LanguageSupport):
    """JavaScript and TypeScript language support."""

    name = "javascript"
    file_extensions = [".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx"]

    def get_analysis_prompt(self) -> str:
        return """You are an expert JavaScript/TypeScript reviewer.

        Focus on these JavaScript-specific issues:
        1. **Async/Await**: Unhandled promise rejections, missing await, async context
        2. **Type Safety** (TypeScript): any usage, type assertions, null/undefined handling
        3. **Security**: XSS, prototype pollution, eval usage, injection vulnerabilities
        4. **Performance**: Event loop blocking, memory leaks, inefficient algorithms
        5. **React** (if applicable): Hook rules, dependency arrays, performance optimizations
        6. **Node.js** (if applicable): Callback patterns, stream handling, error boundaries

        Output using the same JSON schema as other languages.
        Severity levels: critical (security/crashes), high (bugs), medium (best practices), low (style).
        """

    def extract_symbols(self, code: str) -> SymbolGraph:
        """Extract functions, classes, and modules using tree-sitter."""
        parser = self._get_parser()  # Lazy-loaded tree-sitter-javascript
        tree = parser.parse(code.encode())

        symbols = SymbolGraph()
        # Traverse for: function declarations, arrow functions, classes,
        # async functions, generators, React components
        query = self.language.query("""
            (function_declaration name: (identifier) @func.name)
            (variable_declarator
              name: (identifier) @func.name
              value: (arrow_function))
            (class_declaration name: (identifier) @class.name)
        """)

        for match in query.matches(tree.root_node):
            # Process matches into SymbolGraph
            pass

        return symbols

    def doc_style(self) -> DocStyle:
        return DocStyle(
            format="jsdoc",
            example="/** @param {string} name - Description */",
            keywords=["@param", "@returns", "@throws", "@example"]
        )

    def auxiliary_linters(self) -> List[LinterConfig]:
        return [
            LinterConfig(
                name="eslint",
                command=["npx", "eslint", "--format", "json", "{file}"],
                required=False,
                timeout=30,
                severity_map={"error": "high", "warning": "medium"}
            ),
            LinterConfig(
                name="tsc",
                command=["npx", "tsc", "--noEmit", "--incremental", "false", "{file}"],
                required=False,
                timeout=30,
                enabled_for=[".ts", ".tsx"]  # TypeScript only
            )
        ]
```

### 16.2 Integration Example

```python
# In scanner.py after refactoring
async def analyze_file(self, file_path: str, content: str) -> List[Finding]:
    # Detect language
    language = self.registry.detect_language(file_path)
    if not language:
        return []

    # Get language-specific prompt
    prompt = language.get_analysis_prompt()

    # Run LLM analysis
    llm_findings = await self.llm_client.analyze_code_json(
        system_prompt=prompt,
        code=content,
        analyzer=f"code_quality_{language.name}"
    )

    # Optionally run auxiliary linter
    linter_findings = []
    if self.config.languages.use_linters:
        for linter in language.auxiliary_linters():
            if linter.is_available():
                linter_findings.extend(
                    await self.run_linter(linter, file_path)
                )

    # Merge and deduplicate findings
    return self.merge_findings(llm_findings, linter_findings)
```

---

## 17. Integration with Existing Drep Patterns

### 17.1 Architectural Alignment

| Drep Pattern | Language Support Mapping |
| --- | --- |
| Platform adapters (Gitea/GitHub/GitLab) | Language adapters follow same abstract base pattern |
| Finding model | All languages produce same `Finding` structure |
| Cache keys | Extended with language: `{analyzer}:{language}:{repo}:{sha}:{file}` |
| Progress tracking | `ProgressTracker` gains `language` field in callbacks |
| Metrics | `LLMMetrics` extended with `language` tag |

### 17.2 Severity Mapping Preservation
```python
# Consistent across all languages
SEVERITY_MAP = {
    "critical": "error",    # Security vulnerabilities, crashes
    "high": "error",        # Bugs, serious issues
    "medium": "warning",    # Best practices, moderate issues
    "low": "info",          # Minor improvements
    "info": "info"          # Suggestions
}
```

### 17.3 Configuration Compatibility
```yaml
# Existing config remains valid
llm:
  enabled: true
  model: "llama-3.1-70b"

# New section is additive only
languages:
  enabled: ["python"]  # Default preserves current behavior
```

---

## 18. Immediate Next Actions

1. **Approve tree-sitter architecture** (or document alternative) — **blocking Phase 1**
2. **Create implementation tickets:**
   - `LANG-001` Language registry + base abstractions
   - `LANG-002` Python adapter (refactor existing)
   - `LANG-003` Scanner/analyzer refactoring
   - `LANG-004` JavaScript/TypeScript adapter
   - `LANG-005` Go adapter
   - `LANG-006` Rust adapter
   - `LANG-007` Configuration schema updates
   - `LANG-008` Documentation updates
3. **Schedule 2-hour design review** with maintainers
4. **Prepare sample repositories** for each language (test fixtures)
5. **Prototype tree-sitter integration** (Phase 0 spike)

---

## Conclusion

Multi-language support requires **architectural refactoring** rather than simple analyzer additions. This comprehensive plan provides:

- **Clear architecture** with language registry and adapters
- **Phased delivery** (v1.2.0–v1.5.0) maintaining backward compatibility
- **Risk mitigation** strategies for technical and operational challenges
- **Success metrics** to validate implementation quality
- **Cost analysis** showing acceptable resource impact (+20% tokens, +100MB memory)
- **Migration path** ensuring zero disruption for existing Python users

The 46-60 hour estimate remains accurate, with Phase 0 research spike as the critical first step. This plan transforms drep from a Python-only tool into an extensible multi-language platform while preserving all existing functionality.

**Recommendation:** Proceed with Phase 0 (tree-sitter evaluation) to validate the technical approach before committing to full implementation.
