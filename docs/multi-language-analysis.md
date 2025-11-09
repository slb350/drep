# Multi-Language Support Analysis

**Created:** 2025-11-09
**Status:** Investigation Phase
**Goal:** Determine if adding multi-language support requires simple analyzer additions or major refactoring

---

## Executive Summary

**Verdict: MAJOR REFACTORING REQUIRED** (not simple analyzer additions)

Adding multi-language support (JavaScript, TypeScript, Go, Rust) requires significant architectural changes due to deep Python-specific assumptions throughout the codebase. This is a **Phase 4.2** feature with **Large effort** classification.

**Estimated Effort:** 40-60 hours (not 3-4 hours)
- Architecture refactoring: 15-20 hours
- First language (JavaScript/TypeScript): 15-20 hours
- Additional languages: 8-10 hours each
- Testing: 10-15 hours

---

## Current Architecture Analysis

### File Discovery (Scanner)

**Location:** `drep/core/scanner.py`

**Current Implementation:**
```python
# Lines 185-193: Hardcoded Python + Markdown patterns
for pattern in ["**/*.py", "**/*.md"]:
    files.extend([
        str(f.relative_to(repo_path))
        for f in repo_path.glob(pattern)
        if not self._should_ignore(f)
    ])

# Line 251: Git diff filtering
if path and (path.endswith(".py") or path.endswith(".md")):
    changed_files.append(path)

# Line 315: Staged files check
if path and (path.lower().endswith(".py") or path.lower().endswith(".md")):
    staged_files.append(path)
```

**Issue:** File extension checks are scattered and hardcoded
**Refactoring Required:** YES - Need centralized file type registry

---

### Code Quality Analyzer

**Location:** `drep/code_quality/analyzer.py`

**Current Implementation:**
```python
# Lines 17-66: Python-specific analysis prompt
PYTHON_ANALYSIS_PROMPT = """You are an expert Python code reviewer.
Analyze the following code and identify issues in these categories:
...
"""

# Lines 175-187: is_supported_file() method
def is_supported_file(self, file_path: str) -> bool:
    """Check if file is supported for analysis.

    Currently only Python files (.py) are supported.
    """
    path = Path(file_path)
    return path.suffix == ".py"
```

**Issues:**
1. Single hardcoded prompt for Python only
2. No abstraction for language-specific analysis
3. Prompt references PEP 8, Python-specific best practices
4. No concept of language detection or multiple analyzers

**Refactoring Required:** YES - Need language-specific analyzer classes

---

### Docstring Generator (AST Analysis)

**Location:** `drep/docstring/ast_utils.py` and `drep/docstring/generator.py`

**Current Implementation:**
```python
# ast_utils.py uses Python's ast module exclusively
import ast

def extract_functions(code: str) -> List[FunctionInfo]:
    """Extract all function definitions from Python code."""
    tree = ast.parse(code)  # Python AST only
    ...

# Line 175-187: Python-specific AST traversal
for node, parent in _collect_function_nodes(tree):
    if isinstance(parent, (ast.FunctionDef, ast.AsyncFunctionDef)):
        continue
    ...
```

**Issues:**
1. Python `ast` module only works for Python code
2. AST structure is Python-specific (FunctionDef, ClassDef, etc.)
3. Docstring extraction uses `ast.get_docstring()` (Python-only)
4. Each language has different AST parsers and structures

**Refactoring Required:** YES - Need language-specific AST parsers or tree-sitter

---

### Scanner Integration

**Location:** `drep/core/scanner.py:320-422`

**Current Implementation:**
```python
# Line 353: Filter to Python files only
python_files = [f for f in files if self.code_analyzer.is_supported_file(f)]

# Line 457: Hardcoded .py extension check
python_files = [f for f in files if f.endswith(".py")]
```

**Issue:** Assumes single analyzer for all code files
**Refactoring Required:** YES - Need analyzer routing by language

---

## Hardcoded Python Assumptions

### 1. **File Extension Checks** (12 locations)
- `scanner.py` line 185: `["**/*.py", "**/*.md"]`
- `scanner.py` line 251: `.endswith(".py")`
- `scanner.py` line 315: `.lower().endswith(".py")`
- `scanner.py` line 353: `code_analyzer.is_supported_file()`
- `scanner.py` line 457: `.endswith(".py")`
- `code_quality/analyzer.py` line 187: `path.suffix == ".py"`

### 2. **Python-Specific Prompts**
- `code_quality/analyzer.py` lines 17-66: `PYTHON_ANALYSIS_PROMPT`
  - References: PEP 8, Python naming conventions, Python best practices
  - Hardcoded severity levels, categories specific to Python ecosystem

### 3. **AST Parsing**
- `docstring/ast_utils.py`: Entire file (205 lines) is Python AST-specific
  - `ast.parse()` - Python-only parser
  - `ast.FunctionDef`, `ast.AsyncFunctionDef` - Python AST nodes
  - `ast.get_docstring()` - Python docstring extraction
  - Python-specific concepts: decorators, positional-only args, **kwargs

### 4. **File Type Classification**
- `scanner.py` line 139: Comment says "all Python/Markdown files"
- No concept of multiple programming languages
- Documentation files (`.md`) treated separately from code files

---

## Required Architectural Changes

### 1. Language Abstraction Layer

**New Module:** `drep/languages/`

```
drep/languages/
├── __init__.py
├── base.py           # Abstract base for language support
├── python.py         # Python language implementation
├── javascript.py     # JavaScript/TypeScript support
├── go.py            # Go support
└── rust.py          # Rust support
```

**Base Interface:**
```python
from abc import ABC, abstractmethod
from typing import List
from pathlib import Path

class LanguageSupport(ABC):
    """Abstract base for language-specific support."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Language name (e.g., 'python', 'javascript')."""
        pass

    @property
    @abstractmethod
    def file_extensions(self) -> List[str]:
        """Supported file extensions (e.g., ['.py', '.pyi'])."""
        pass

    @abstractmethod
    def get_analysis_prompt(self) -> str:
        """Get language-specific LLM analysis prompt."""
        pass

    @abstractmethod
    def extract_functions(self, code: str) -> List[FunctionInfo]:
        """Extract functions from code (AST or regex-based)."""
        pass

    @abstractmethod
    def extract_classes(self, code: str) -> List[ClassInfo]:
        """Extract classes from code."""
        pass

    @abstractmethod
    def is_comment(self, line: str) -> bool:
        """Check if line is a comment."""
        pass

    @abstractmethod
    def get_linter_config(self) -> dict:
        """Get external linter configuration (ESLint, rustfmt, etc.)."""
        pass
```

### 2. Language Registry

**New Module:** `drep/languages/registry.py`

```python
from typing import Dict, Optional
from pathlib import Path
from drep.languages.base import LanguageSupport
from drep.languages.python import PythonSupport
from drep.languages.javascript import JavaScriptSupport

class LanguageRegistry:
    """Central registry for language support."""

    def __init__(self):
        self._languages: Dict[str, LanguageSupport] = {}
        self._extension_map: Dict[str, str] = {}

        # Register built-in languages
        self.register(PythonSupport())
        self.register(JavaScriptSupport())

    def register(self, language: LanguageSupport):
        """Register a language."""
        self._languages[language.name] = language
        for ext in language.file_extensions:
            self._extension_map[ext] = language.name

    def detect_language(self, file_path: str) -> Optional[LanguageSupport]:
        """Detect language from file extension."""
        ext = Path(file_path).suffix
        if lang_name := self._extension_map.get(ext):
            return self._languages[lang_name]
        return None

    def get_supported_extensions(self) -> List[str]:
        """Get all supported file extensions."""
        return list(self._extension_map.keys())
```

### 3. Multi-Language Code Analyzer

**Refactor:** `drep/code_quality/analyzer.py`

```python
class CodeQualityAnalyzer:
    """Multi-language code quality analyzer."""

    def __init__(self, llm_client: LLMClient, language_registry: LanguageRegistry):
        self.llm_client = llm_client
        self.language_registry = language_registry

    async def analyze_file(
        self, file_path: str, content: str, repo_id: str, commit_sha: str
    ) -> List[Finding]:
        """Analyze file with language-specific logic."""

        # Detect language
        language = self.language_registry.detect_language(file_path)
        if not language:
            logger.debug(f"Unsupported language for {file_path}")
            return []

        # Get language-specific prompt
        prompt = language.get_analysis_prompt()

        # Call LLM with language-specific prompt
        result_dict = await self.llm_client.analyze_code_json(
            system_prompt=prompt,
            code=content,
            schema=CodeAnalysisResult,
            repo_id=repo_id,
            commit_sha=commit_sha,
            analyzer=f"code_quality_{language.name}",
        )

        # Convert to findings
        result = CodeAnalysisResult(**result_dict)
        return result.to_findings(file_path)

    def is_supported_file(self, file_path: str) -> bool:
        """Check if file is supported."""
        return self.language_registry.detect_language(file_path) is not None
```

### 4. Scanner Refactoring

**Refactor:** `drep/core/scanner.py`

```python
class RepositoryScanner:
    def __init__(self, db_session, config, language_registry: LanguageRegistry):
        self.db = db_session
        self.config = config
        self.language_registry = language_registry
        # ...

    def _get_all_code_files(self, repo_path: str) -> List[str]:
        """Get all code files in supported languages."""
        files = []
        repo_path = Path(repo_path)

        # Get patterns for all supported languages
        extensions = self.language_registry.get_supported_extensions()
        patterns = [f"**/*{ext}" for ext in extensions]

        # Add markdown (documentation)
        patterns.append("**/*.md")

        for pattern in patterns:
            files.extend([
                str(f.relative_to(repo_path))
                for f in repo_path.glob(pattern)
                if not self._should_ignore(f)
            ])

        return files

    def _get_changed_files(self, repo: Repo, old_sha: str, new_sha: str) -> List[str]:
        """Get changed files in supported languages."""
        diff = repo.commit(old_sha).diff(new_sha)

        supported_extensions = set(self.language_registry.get_supported_extensions())
        supported_extensions.add(".md")  # Always include markdown

        changed_files = []
        for diff_item in diff:
            path = diff_item.b_path
            if path and Path(path).suffix in supported_extensions:
                changed_files.append(path)

        return list(set(changed_files))
```

### 5. Docstring/AST Abstraction

**Challenge:** Each language has different AST structure

**Options:**

#### Option A: Tree-sitter (Recommended)
Use tree-sitter for unified AST parsing across languages:
- **Pros:** Single interface, supports 50+ languages, fast
- **Cons:** Additional C dependency, learning curve
- **Libraries:** `tree-sitter`, language-specific grammars

```python
from tree_sitter import Language, Parser

class PythonSupport(LanguageSupport):
    def __init__(self):
        self.parser = Parser()
        self.parser.set_language(Language('build/languages.so', 'python'))

    def extract_functions(self, code: str) -> List[FunctionInfo]:
        tree = self.parser.parse(bytes(code, 'utf8'))
        # Query for function definitions
        query = self.language.query("""
            (function_definition
                name: (identifier) @name
                parameters: (parameters) @params
                body: (block) @body)
        """)
        matches = query.captures(tree.root_node)
        # Process matches...
```

#### Option B: Language-Specific Parsers
Use native parsers for each language:
- **Python:** `ast` module (already implemented)
- **JavaScript/TypeScript:** `esprima` or `@typescript-eslint/parser` (via subprocess)
- **Go:** `go/ast` package (via subprocess)
- **Rust:** `syn` crate (via subprocess)

**Pros:** Native tooling, accurate parsing
**Cons:** Complex integration, multiple dependencies, subprocess overhead

#### Option C: Regex-Based (Fallback)
Use regex patterns for simple extraction:
- **Pros:** No dependencies, fast
- **Cons:** Fragile, misses edge cases, not suitable for complex analysis

**Recommendation:** Option A (tree-sitter) for unified approach

---

## Language-Specific Considerations

### JavaScript/TypeScript

**File Extensions:** `.js`, `.jsx`, `.ts`, `.tsx`, `.mjs`, `.cjs`

**Analysis Prompt Additions:**
- ESLint rules integration
- TypeScript-specific: type errors, any usage, strict mode
- React-specific: hooks rules, JSX best practices
- Node.js vs Browser context

**AST Challenges:**
- ES6+ syntax (arrow functions, destructuring, async/await)
- JSX syntax (requires special parser)
- TypeScript types and generics
- Module systems: CommonJS, ES modules, AMD

**Docstring Format:**
- JSDoc comments (`/** ... */`)
- TypeScript interfaces and type definitions

### Go

**File Extensions:** `.go`

**Analysis Prompt Additions:**
- `go vet` integration
- gofmt style compliance
- Error handling patterns (no exceptions)
- Goroutine safety, race conditions
- Interface satisfaction

**AST Challenges:**
- Go's unique syntax (defer, goroutines, channels)
- Package vs module system
- Implicit interfaces

**Docstring Format:**
- Standard Go comments (no special syntax)
- Package-level documentation

### Rust

**File Extensions:** `.rs`

**Analysis Prompt Additions:**
- Clippy lints integration
- Ownership and borrowing rules
- Unsafe code blocks
- Lifetime annotations
- Cargo.toml integration

**AST Challenges:**
- Complex type system (traits, lifetimes, generics)
- Macros (require macro expansion)
- Procedural macros

**Docstring Format:**
- Doc comments (`///` for items, `//!` for modules)
- Markdown support in docs

---

## Configuration Changes

### config.yaml Extensions

```yaml
# New section for language support
languages:
  enabled:
    - python
    - javascript
    - typescript
    - go
    - rust

  # Language-specific settings
  python:
    linter: pylint  # or ruff, flake8
    style_guide: pep8

  javascript:
    linter: eslint
    parser: babel  # or espree

  typescript:
    linter: eslint
    parser: typescript-eslint

  go:
    linter: golangci-lint
    formatter: gofmt

  rust:
    linter: clippy
    formatter: rustfmt

# Existing LLM config remains unchanged
llm:
  enabled: true
  ...
```

### Backward Compatibility

**Current behavior:** Only Python files analyzed
**New behavior:** Analyze all enabled languages (default: Python only)

**Migration Path:**
1. v1.2.0: Add language registry, default to Python only
2. v1.3.0: Add JavaScript/TypeScript support (opt-in)
3. v1.4.0: Add Go/Rust support (opt-in)

---

## Testing Strategy

### Unit Tests Per Language (est. 15-20 tests each)

```python
# tests/unit/languages/test_python_support.py
def test_python_file_detection():
    lang = PythonSupport()
    assert ".py" in lang.file_extensions
    assert lang.name == "python"

def test_python_function_extraction():
    code = "def foo(x: int) -> str:\n    return str(x)"
    lang = PythonSupport()
    functions = lang.extract_functions(code)
    assert len(functions) == 1
    assert functions[0].name == "foo"

# tests/unit/languages/test_javascript_support.py
def test_javascript_arrow_function_extraction():
    code = "const foo = (x) => x * 2;"
    lang = JavaScriptSupport()
    functions = lang.extract_functions(code)
    assert len(functions) == 1
    assert functions[0].name == "foo"
```

### Integration Tests (est. 10 tests)

```python
@pytest.mark.integration
async def test_multi_language_repository_scan():
    """Test scanning repository with Python, JS, and Go files."""
    # Create test repo with mixed languages
    repo = create_mixed_language_repo()

    scanner = RepositoryScanner(db, config, language_registry)
    files, sha = await scanner.scan_repository(repo.path, "test", "mixed")

    # Verify all languages detected
    python_files = [f for f in files if f.endswith(".py")]
    js_files = [f for f in files if f.endswith(".js")]
    go_files = [f for f in files if f.endswith(".go")]

    assert len(python_files) > 0
    assert len(js_files) > 0
    assert len(go_files) > 0
```

---

## Migration Plan

### Phase 1: Foundation (v1.2.0) - 15-20 hours

**Goal:** Add language abstraction without breaking changes

**Tasks:**
1. Create `drep/languages/` module structure
2. Implement `LanguageSupport` base class
3. Implement `LanguageRegistry`
4. Migrate Python support to `PythonSupport` class
5. Refactor `CodeQualityAnalyzer` to use language registry
6. Refactor `Scanner` to use language registry (backward compatible)
7. Add 20+ unit tests for language abstraction
8. Update documentation

**Deliverables:**
- `drep/languages/base.py`
- `drep/languages/registry.py`
- `drep/languages/python.py`
- Tests passing (795 existing + 20 new = 815)
- Zero breaking changes (Python-only behavior preserved)

### Phase 2: JavaScript/TypeScript (v1.3.0) - 15-20 hours

**Goal:** Add first additional language

**Tasks:**
1. Choose AST parser (tree-sitter vs esprima)
2. Implement `JavaScriptSupport` class
3. Add `.js`, `.jsx`, `.ts`, `.tsx` support
4. Create JavaScript-specific analysis prompt
5. Add ESLint integration (optional)
6. Implement function/class extraction for JS
7. Add 20+ tests for JavaScript support
8. Update configuration schema
9. Update documentation

**Deliverables:**
- `drep/languages/javascript.py`
- JavaScript analysis working end-to-end
- Tests: 835 total (815 + 20 new)

### Phase 3: Go Support (v1.4.0) - 8-10 hours

**Tasks:**
1. Implement `GoSupport` class
2. Add `.go` file support
3. Create Go-specific analysis prompt
4. Add golangci-lint integration (optional)
5. Implement function/struct extraction for Go
6. Add 15+ tests
7. Update documentation

**Deliverables:**
- `drep/languages/go.py`
- Tests: 850 total (835 + 15 new)

### Phase 4: Rust Support (v1.5.0) - 8-10 hours

**Tasks:**
1. Implement `RustSupport` class
2. Add `.rs` file support
3. Create Rust-specific analysis prompt
4. Add clippy integration (optional)
5. Implement function/impl extraction for Rust
6. Add 15+ tests
7. Update documentation

**Deliverables:**
- `drep/languages/rust.py`
- Tests: 865 total (850 + 15 new)

---

## Risks and Challenges

### 1. AST Parsing Complexity
**Risk:** Each language has vastly different syntax and AST structure
**Mitigation:** Use tree-sitter for unified interface, fall back to regex for simple cases

### 2. External Linter Integration
**Risk:** Depending on external tools (ESLint, clippy) adds dependencies
**Mitigation:** Make external linters optional, provide pure LLM-based analysis as fallback

### 3. LLM Prompt Quality
**Risk:** LLM may not be equally good at all languages
**Mitigation:** Test prompts with real code samples, iterate on prompt engineering

### 4. Performance Degradation
**Risk:** Supporting many languages could slow down scans
**Mitigation:** Parallel analysis (already implemented), language-specific caching

### 5. Breaking Changes
**Risk:** Refactoring could break existing users
**Mitigation:** Strict backward compatibility testing, default to Python-only

---

## Conclusion

### Simple Analyzer Addition? **NO**

Adding multi-language support is **NOT** as simple as adding new analyzer classes. It requires:

1. ✅ **Language abstraction layer** (new architecture)
2. ✅ **Refactoring file discovery** (scanner changes)
3. ✅ **Refactoring analyzer dispatch** (routing by language)
4. ✅ **Language-specific AST parsing** (tree-sitter or per-language parsers)
5. ✅ **Configuration schema changes** (language-specific settings)
6. ✅ **Comprehensive testing** (per-language test suites)

### Recommendation

**Phase 4.2 Status:** Accurate ("Large effort, High impact")

**Proposed Timeline:**
- v1.2.0 (Phase 1): Language abstraction foundation (15-20 hours)
- v1.3.0 (Phase 2): JavaScript/TypeScript support (15-20 hours)
- v1.4.0 (Phase 3): Go support (8-10 hours)
- v1.5.0 (Phase 4): Rust support (8-10 hours)

**Total Effort:** 46-60 hours (spread across 4 releases)

**Priority:** After Anthropic Direct provider (Phase 4.1, 3-4 hours)

### Next Steps

1. Review this analysis with stakeholders
2. Approve architectural approach (tree-sitter vs native parsers)
3. Create Phase 4.2 implementation plan
4. Begin Phase 1 (foundation) work
5. Prototype JavaScript support to validate approach

---

## References

- Tree-sitter: https://tree-sitter.github.io/tree-sitter/
- ESLint Parser: https://github.com/eslint/espree
- Go AST: https://pkg.go.dev/go/ast
- Rust syn: https://docs.rs/syn/latest/syn/
- Current Python implementation: `drep/code_quality/analyzer.py`, `drep/docstring/ast_utils.py`
