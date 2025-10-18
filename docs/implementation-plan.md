# Implementation Plan: drep MVP

**Document Version:** 1.4
**Last Updated:** 2025-10-18
**Status:** Phase 1, 2 & 3 Complete - Ready for Phase 4

This document provides a detailed, step-by-step implementation plan for building drep MVP (Gitea + Python only).

---

## Overview

**Goal:** Build a working `drep scan owner/repo` command that:
1. Scans Gitea Python repositories
2. Detects typos and pattern issues in documentation
3. Creates issues on Gitea with findings
4. Supports incremental scanning

**Scope:**
- ✅ Gitea only (no GitHub/GitLab)
- ✅ Python only (no JavaScript/TypeScript)
- ✅ Layer 1 + 2 only (spellcheck + patterns, no LLM)
- ✅ CLI only (no webhooks)

---

## Implementation Phases

### Phase 1: Foundation ✅ COMPLETE
**Purpose:** Set up project structure, data models, and configuration
**Status:** Complete - All tests passing (60/60)

### Phase 2: Gitea Integration ✅ COMPLETE
**Purpose:** Connect to Gitea API and handle authentication
**Status:** Complete - All tests passing (78/78, +18 new tests)

### Phase 3: Documentation Analysis ✅ COMPLETE
**Purpose:** Build spellcheck and pattern detection
**Status:** Complete - All tests passing (117/117, +39 new tests)
**Bug Fixes:** 5 additional bugs fixed during code review (4 Medium + 1 High severity)

### Phase 4: Repository Scanning
**Purpose:** Clone repos and scan files incrementally

### Phase 5: Issue Management
**Purpose:** Create issues with deduplication

### Phase 6: CLI & Integration
**Purpose:** Wire everything together with CLI commands

### Phase 7: Testing & Polish
**Purpose:** Test, fix bugs, and prepare for release

---

## Phase 1: Foundation

### 1.1 Project Setup ✅

**COMPLETED:**
- [x] Ensure Python 3.10+ is installed
- [x] Create virtual environment: `python -m venv venv`
- [x] Activate virtual environment
- [x] Update `pyproject.toml` with correct dependencies (if needed)
- [x] Install package in development mode: `pip install -e .`
- [x] Verify installation: `python -c "import drep; print(drep.__version__)"`

**Expected Output:**
```
$ python -c "import drep; print(drep.__version__)"
0.1.0
```

**Guidelines:**
- Use virtual environment to avoid polluting system Python
- Pin dependency versions in `pyproject.toml` for reproducibility
- Test imports after installation to catch early errors

---

### 1.2 Configuration Models ✅

**File:** `drep/models/config.py`

**COMPLETED:**
- [x] Import required Pydantic classes: `BaseModel`, `Field`
- [x] Create `GiteaConfig` class with fields: `url`, `token`, `repositories`
- [x] Create `DocumentationConfig` class with fields: `enabled`, `custom_dictionary`
- [x] Create `Config` class that combines all sub-configs
- [x] Add field descriptions using `Field(..., description="...")`
- [x] Add default values where appropriate (e.g., `enabled: bool = True`)
- [x] Test model validation with sample data (8 tests passing)

**Code Structure:**
```python
from pydantic import BaseModel, Field
from typing import List

class GiteaConfig(BaseModel):
    """Gitea platform configuration."""
    url: str = Field(..., description="Gitea base URL (e.g., http://192.168.1.14:3000)")
    token: str = Field(..., description="Gitea API token")
    repositories: List[str] = Field(..., description="Repository patterns (e.g., steve/*)")

class DocumentationConfig(BaseModel):
    """Documentation analysis settings."""
    enabled: bool = True
    custom_dictionary: List[str] = Field(default_factory=list)

class Config(BaseModel):
    """Main configuration."""
    gitea: GiteaConfig
    documentation: DocumentationConfig
    database_url: str = "sqlite:///./drep.db"
```

**Testing:**
```python
# Quick validation test
config_dict = {
    "gitea": {
        "url": "http://192.168.1.14:3000",
        "token": "test_token",
        "repositories": ["steve/*"]
    },
    "documentation": {
        "enabled": True,
        "custom_dictionary": ["asyncio"]
    }
}
config = Config(**config_dict)
print(config.gitea.url)  # Should print URL
```

**Guidelines:**
- Use descriptive field names
- Provide clear descriptions for all fields
- Use `Field(default_factory=list)` for mutable defaults (never `[]` directly)
- Validate by creating test instances

---

### 1.3 Finding Models ✅

**File:** `drep/models/findings.py`

**COMPLETED:**
- [x] Create `Typo` class with fields: `word`, `replacement`, `line`, `column`, `context`, `suggestions`
- [x] Create `PatternIssue` class with fields: `type`, `line`, `column`, `matched_text`, `replacement`
- [x] Create `Finding` class (generic) with fields: `type`, `severity`, `file_path`, `line`, `column`, `original`, `replacement`, `message`, `suggestion`
- [x] Create `DocumentationFindings` class with lists of typos and patterns
- [x] Implement `to_findings()` method on `DocumentationFindings`
- [x] Test model serialization/deserialization (15 tests passing)
- [x] Fixed mutable default issues using Field(default_factory=list)

**Code Structure:**
```python
from pydantic import BaseModel
from typing import Optional, List

class Typo(BaseModel):
    word: str
    replacement: str
    line: int
    column: int
    context: str
    suggestions: List[str] = []

class PatternIssue(BaseModel):
    type: str
    line: int
    column: int
    matched_text: str
    replacement: str

class Finding(BaseModel):
    type: str
    severity: str
    file_path: str
    line: int
    column: Optional[int] = None
    original: Optional[str] = None
    replacement: Optional[str] = None
    message: str
    suggestion: Optional[str] = None

class DocumentationFindings(BaseModel):
    file_path: str
    typos: List[Typo] = []
    pattern_issues: List[PatternIssue] = []

    def to_findings(self) -> List[Finding]:
        # Convert to generic Finding objects
        pass
```

**Testing:**
```python
# Test Typo model
typo = Typo(
    word="teh",
    replacement="the",
    line=5,
    column=10,
    context="This is teh test",
    suggestions=["the", "tea"]
)
print(typo.word)  # Should print "teh"

# Test to_findings() conversion
findings = DocumentationFindings(file_path="test.py")
findings.typos.append(typo)
generic_findings = findings.to_findings()
assert len(generic_findings) == 1
assert generic_findings[0].type == 'typo'
```

**Guidelines:**
- Keep models simple and focused
- Use explicit field types (avoid `Any`)
- Test serialization to/from JSON
- Implement `to_findings()` carefully - this is critical for issue creation

---

### 1.4 Database Models ✅

**File:** `drep/db/models.py`

**COMPLETED:**
- [x] Import SQLAlchemy: `Column`, `Integer`, `String`, `DateTime`
- [x] Import `declarative_base` (from sqlalchemy.orm)
- [x] Create `Base` declarative base
- [x] Create `RepositoryScan` table with columns: `id`, `owner`, `repo`, `commit_sha`, `scanned_at`
- [x] Create `FindingCache` table with columns: `id`, `owner`, `repo`, `file_path`, `finding_hash`, `issue_number`, `created_at`
- [x] Add indexes for common queries (owner+repo lookup)
- [x] Add UniqueConstraint for (owner, repo) to prevent duplicates
- [x] Test table creation (13 tests passing)
- [x] Updated to SQLAlchemy 2.0 and datetime.now(UTC)

**Code Structure:**
```python
from sqlalchemy import Column, Integer, String, DateTime, Index
from sqlalchemy.ext.declarative import declarative_base
from datetime import datetime

Base = declarative_base()

class RepositoryScan(Base):
    __tablename__ = 'repository_scans'

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    commit_sha = Column(String, nullable=False)
    scanned_at = Column(DateTime, default=datetime.utcnow)

    # Index for faster lookups
    __table_args__ = (
        Index('idx_owner_repo', 'owner', 'repo'),
    )

class FindingCache(Base):
    __tablename__ = 'finding_cache'

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    file_path = Column(String, nullable=False)
    finding_hash = Column(String, nullable=False, unique=True)
    issue_number = Column(Integer, nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)

    __table_args__ = (
        Index('idx_finding_hash', 'finding_hash'),
    )
```

**Testing:**
```python
from sqlalchemy import create_engine
from drep.db.models import Base

# Test table creation
engine = create_engine('sqlite:///test.db')
Base.metadata.create_all(engine)
# Should create tables without errors
```

**Guidelines:**
- Use meaningful table names (plural)
- Always add indexes for foreign key lookups
- Use `datetime.utcnow` for timestamps (not `now()`)
- Test table creation before moving on

---

### 1.5 Database Initialization ✅

**File:** `drep/db/__init__.py`

**COMPLETED:**
- [x] Create `init_database()` function that takes `database_url`
- [x] Create engine with SQLAlchemy
- [x] Create all tables using `Base.metadata.create_all()`
- [x] Return database session
- [x] Handle errors gracefully (7 tests passing)

**Code Structure:**
```python
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker
from drep.db.models import Base

def init_database(database_url: str):
    """Initialize database and return session."""
    engine = create_engine(database_url)
    Base.metadata.create_all(engine)
    Session = sessionmaker(bind=engine)
    return Session()
```

**Testing:**
```python
from drep.db import init_database

# Test database initialization
session = init_database('sqlite:///test.db')
# Should create drep.db and return session
```

**Guidelines:**
- Use `create_all()` (idempotent - safe to run multiple times)
- Return session, not engine (easier to use)
- Add error handling for permission issues

---

### 1.6 Configuration Loading ✅

**File:** `drep/config.py` (new file)

**COMPLETED:**
- [x] Create `load_config()` function that takes `config_path`
- [x] Read YAML file
- [x] Substitute environment variables (e.g., `${GITEA_TOKEN}`)
- [x] Parse with Pydantic `Config` model
- [x] Return validated `Config` object
- [x] Handle file not found errors
- [x] Handle YAML parsing errors
- [x] Handle validation errors
- [x] Add strict mode for production env var validation
- [x] Add empty/malformed YAML detection with clear errors (17 tests passing)

**Code Structure:**
```python
import yaml
import os
import re
from pathlib import Path
from drep.models.config import Config

def load_config(config_path: str) -> Config:
    """Load and validate configuration from YAML file."""
    config_file = Path(config_path)

    if not config_file.exists():
        raise FileNotFoundError(f"Config file not found: {config_path}")

    # Read YAML
    with config_file.open() as f:
        raw_config = yaml.safe_load(f)

    # Substitute environment variables
    config_str = yaml.dump(raw_config)
    config_str = _substitute_env_vars(config_str)
    config_dict = yaml.safe_load(config_str)

    # Validate with Pydantic
    return Config(**config_dict)

def _substitute_env_vars(text: str) -> str:
    """Replace ${VAR_NAME} with environment variable values."""
    pattern = r'\$\{([^}]+)\}'

    def replacer(match):
        var_name = match.group(1)
        return os.environ.get(var_name, match.group(0))

    return re.sub(pattern, replacer, text)
```

**Testing:**
```bash
# Set environment variable
export GITEA_TOKEN="test_token_123"

# Create test config.yaml
cat > test_config.yaml << EOF
gitea:
  url: http://192.168.1.14:3000
  token: \${GITEA_TOKEN}
  repositories:
    - steve/*
documentation:
  enabled: true
EOF

# Test loading
python -c "from drep.config import load_config; c = load_config('test_config.yaml'); print(c.gitea.token)"
# Should print: test_token_123
```

**Guidelines:**
- Use `yaml.safe_load()` (not `load()` - security)
- Always validate with Pydantic after loading
- Provide helpful error messages
- Support environment variable substitution

---

## Phase 2: Gitea Integration

### 2.1 Gitea Adapter - Basic Structure ✅

**File:** `drep/adapters/gitea.py`

**COMPLETED:**
- [x] Import `httpx`, `List`, `Optional`
- [x] Create `GiteaAdapter` class with `__init__(url, token)`
- [x] Initialize `httpx.AsyncClient` with authorization header
- [x] Add `close()` method to close HTTP client
- [x] Test instantiation (5 tests passing)

**Code Structure:**
```python
import httpx
from typing import List, Optional

class GiteaAdapter:
    """Gitea API adapter."""

    def __init__(self, url: str, token: str):
        self.url = url.rstrip('/')
        self.token = token
        self.client = httpx.AsyncClient(
            headers={'Authorization': f'token {token}'},
            timeout=30.0
        )

    async def close(self):
        """Close HTTP client connection."""
        await self.client.aclose()
```

**Testing:**
```python
import asyncio

async def test():
    adapter = GiteaAdapter("http://192.168.1.14:3000", "test_token")
    print(f"Client created: {adapter.client is not None}")
    await adapter.close()

asyncio.run(test())
```

**Guidelines:**
- Set reasonable timeout (30s default)
- Store URL without trailing slash (consistent)
- Always provide `close()` method for cleanup

---

### 2.2 Gitea Adapter - Get Default Branch ✅

**File:** `drep/adapters/gitea.py`

**COMPLETED:**
- [x] Add `get_default_branch(owner, repo)` async method
- [x] Build API URL: `/api/v1/repos/{owner}/{repo}`
- [x] Make GET request
- [x] Handle HTTP errors (404, 401, etc.)
- [x] Extract `default_branch` from JSON response
- [x] Return branch name
- [x] Test with HTTP mocking (5 tests passing)

**Code to Add:**
```python
async def get_default_branch(self, owner: str, repo: str) -> str:
    """Get repository default branch."""
    url = f"{self.url}/api/v1/repos/{owner}/{repo}"

    try:
        response = await self.client.get(url)
        response.raise_for_status()
        data = response.json()
        return data['default_branch']
    except httpx.HTTPStatusError as e:
        if e.response.status_code == 404:
            raise ValueError(f"Repository {owner}/{repo} not found")
        elif e.response.status_code == 401:
            raise ValueError("Unauthorized - check your Gitea token")
        else:
            raise
```

**Testing:**
```python
async def test():
    adapter = GiteaAdapter("http://192.168.1.14:3000", "YOUR_TOKEN")
    try:
        branch = await adapter.get_default_branch("steve", "drep")
        print(f"Default branch: {branch}")
    finally:
        await adapter.close()

asyncio.run(test())
# Should print: Default branch: main (or master)
```

**Guidelines:**
- Handle common HTTP errors explicitly (404, 401)
- Provide helpful error messages
- Test with real repository before moving on

---

### 2.3 Gitea Adapter - Create Issue ✅

**File:** `drep/adapters/gitea.py`

**COMPLETED:**
- [x] Add `create_issue(owner, repo, title, body, labels)` async method
- [x] Build API URL: `/api/v1/repos/{owner}/{repo}/issues`
- [x] Build request payload with title, body, labels
- [x] Make POST request
- [x] Handle HTTP errors
- [x] Extract issue number from response
- [x] Return issue number
- [x] Test with HTTP mocking (4 tests passing)

**Code to Add:**
```python
async def create_issue(
    self,
    owner: str,
    repo: str,
    title: str,
    body: str,
    labels: List[str] = None
) -> int:
    """Create an issue and return issue number."""
    url = f"{self.url}/api/v1/repos/{owner}/{repo}/issues"
    payload = {
        'title': title,
        'body': body,
        'labels': labels or []
    }

    try:
        response = await self.client.post(url, json=payload)
        response.raise_for_status()
        data = response.json()
        return data['number']
    except httpx.HTTPStatusError as e:
        raise ValueError(f"Failed to create issue: {e.response.text}")
```

**Testing:**
```python
async def test():
    adapter = GiteaAdapter("http://192.168.1.14:3000", "YOUR_TOKEN")
    try:
        issue_num = await adapter.create_issue(
            "steve",
            "test-repo",
            "[Test] drep issue creation",
            "This is a test issue created by drep",
            ["documentation", "automated"]
        )
        print(f"Created issue #{issue_num}")
    finally:
        await adapter.close()

asyncio.run(test())
# Should create issue and print issue number
```

**Guidelines:**
- Use descriptive error messages
- Include response text in errors for debugging
- Test with real repository (check Gitea UI to verify)
- Clean up test issues after testing

---

## Phase 3: Documentation Analysis ✅ COMPLETE

### 3.1 Spellcheck Layer - Basic Structure ✅

**File:** `drep/documentation/spellcheck.py`

**COMPLETED:**
- [x] Import `SpellChecker` from `pyspellchecker`
- [x] Import regex, Path, List
- [x] Import `Typo` model
- [x] Create `SpellcheckLayer` class with `__init__(custom_words)`
- [x] Initialize `SpellChecker` instance
- [x] Load custom words if provided
- [x] Create stub `check()` method
- [x] 3 tests passing

**Code Structure:**
```python
import re
from spellchecker import SpellChecker
from pathlib import Path
from typing import List
from drep.models.findings import Typo

class SpellcheckLayer:
    """Layer 1: Dictionary spellcheck with context awareness."""

    def __init__(self, custom_words: List[str] = None):
        self.spell = SpellChecker()
        if custom_words:
            self.spell.word_frequency.load_words(custom_words)

    def check(self, text: str, file_path: str = "") -> List[Typo]:
        """Check text for typos with context awareness."""
        # To be implemented
        return []
```

**Testing:**
```python
from drep.documentation.spellcheck import SpellcheckLayer

layer = SpellcheckLayer(custom_words=["gitea", "drep"])
print("Spellcheck layer initialized")
```

**Guidelines:**
- Load custom dictionary in `__init__` (one-time setup)
- Keep `check()` signature consistent (takes text + file_path)

---

### 3.2 Spellcheck Layer - Check Line ✅

**File:** `drep/documentation/spellcheck.py`

**COMPLETED:**
- [x] Add `_check_line(line, line_num)` private method
- [x] Remove URLs from line using regex
- [x] Remove inline code backticks using regex
- [x] Extract words (alphabetic only) using regex
- [x] Get misspelled words from spell checker
- [x] Filter out code identifiers (camelCase, snake_case, numbers)
- [x] For each misspelled word, create `Typo` object
- [x] Return list of typos
- [x] Test with sample text
- [x] 7 tests passing

**Code to Add:**
```python
def _check_line(self, line: str, line_num: int) -> List[Typo]:
    """Check a single line of text."""
    typos = []

    # Remove URLs
    line_no_urls = re.sub(r'https?://\S+', '', line)

    # Remove inline code `like this`
    line_no_code = re.sub(r'`[^`]+`', '', line_no_urls)

    # Extract words (alphabetic only)
    words = re.findall(r'\b[a-zA-Z]+\b', line_no_code)

    misspelled = self.spell.unknown(words)

    for word in misspelled:
        # Skip if it looks like a variable name
        if self._is_identifier(word):
            continue

        # Get suggestions
        suggestions = self.spell.candidates(word)
        replacement = list(suggestions)[0] if suggestions else word

        # Find column
        column = line.find(word)

        typos.append(Typo(
            word=word,
            replacement=replacement,
            line=line_num,
            column=column,
            context=line.strip(),
            suggestions=list(suggestions)[:5] if suggestions else []
        ))

    return typos

def _is_identifier(self, word: str) -> bool:
    """Check if word looks like a code identifier."""
    # camelCase
    if re.match(r'^[a-z]+[A-Z]', word):
        return True
    # Has numbers
    if re.search(r'\d', word):
        return True
    return False
```

**Testing:**
```python
layer = SpellcheckLayer()

# Test with typo
typos = layer._check_line("This is a teh test", 1)
assert len(typos) == 1
assert typos[0].word == "teh"
assert "the" in typos[0].suggestions

# Test filtering code identifiers
typos = layer._check_line("The myVariable is camelCase", 1)
assert len(typos) == 0  # Should filter out camelCase identifiers

# Test URL filtering
typos = layer._check_line("Check https://example.com for teh docs", 1)
assert len(typos) == 1  # Only 'teh', not URL
```

**Guidelines:**
- Use regex for cleaning (URLs, code blocks)
- Always filter code identifiers to reduce false positives
- Test edge cases (URLs, code, camelCase)

---

### 3.3 Spellcheck Layer - Check Markdown ✅

**File:** `drep/documentation/spellcheck.py`

**COMPLETED:**
- [x] Add `_check_markdown(text)` method
- [x] Split text into lines
- [x] Track whether in code block (starts with ```)
- [x] Toggle `in_code_block` flag when encountering ```
- [x] Only check lines that are NOT in code blocks
- [x] Use `_check_line()` for each prose line
- [x] Test with sample markdown containing code blocks
- [x] 3 tests passing

**Code to Add:**
```python
def _check_markdown(self, text: str) -> List[Typo]:
    """Check markdown, skipping code blocks."""
    typos = []
    lines = text.split('\n')
    in_code_block = False

    for line_num, line in enumerate(lines, 1):
        if line.strip().startswith('```'):
            in_code_block = not in_code_block
            continue

        if not in_code_block:
            # Check this line as prose
            line_typos = self._check_line(line, line_num)
            typos.extend(line_typos)

    return typos
```

**Testing:**
```python
markdown_text = """# Title

This has a teh typo.

```python
# This code has teh typo but should be ignored
def test():
    pass
```

Another teh typo in prose.
"""

layer = SpellcheckLayer()
typos = layer._check_markdown(markdown_text)

# Should find 2 typos (line 3 and line 10), not the one in code block
assert len(typos) == 2
assert all(t.word == "teh" for t in typos)
```

**Guidelines:**
- Track code block state with boolean flag
- Use `startswith('```')` to detect fences
- Toggle flag (works for both opening and closing)
- Test with nested or unclosed code blocks

---

### 3.4 Spellcheck Layer - Check Python Comments ✅

**File:** `drep/documentation/spellcheck.py`

**COMPLETED:**
- [x] Add `_check_python_comments(text)` method
- [x] Use tokenize module to extract real comments (not string literals)
- [x] For each COMMENT token, extract comment text and check
- [x] Use AST to extract docstrings from Module/Function/Class nodes
- [x] Check docstrings using `_check_plain_text()`
- [x] Handle syntax errors and tokenization failures gracefully
- [x] Accurate line/column tracking for all comments and docstrings
- [x] Test with Python file containing comments and docstrings
- [x] 8 tests passing (including edge cases)

**Code to Add:**
```python
def _check_python_comments(self, text: str) -> List[Typo]:
    """Check Python comments and docstrings only."""
    import ast
    typos = []

    # Extract comments (lines with #)
    lines = text.split('\n')
    for line_num, line in enumerate(lines, 1):
        if '#' in line:
            comment = line.split('#', 1)[1]
            comment_typos = self._check_line(comment, line_num)
            typos.extend(comment_typos)

    # Extract docstrings using AST
    try:
        tree = ast.parse(text)
        for node in ast.walk(tree):
            docstring = ast.get_docstring(node)
            if docstring:
                # Simple line number estimate
                line_num = node.lineno if hasattr(node, 'lineno') else 1
                doc_typos = self._check_plain_text(docstring)
                for typo in doc_typos:
                    typo.line = line_num
                typos.extend(doc_typos)
    except SyntaxError:
        pass  # Skip files with syntax errors

    return typos

def _check_plain_text(self, text: str) -> List[Typo]:
    """Check plain text (for docstrings)."""
    typos = []
    lines = text.split('\n')

    for line_num, line in enumerate(lines, 1):
        line_typos = self._check_line(line, line_num)
        typos.extend(line_typos)

    return typos
```

**Testing:**
```python
python_code = '''
def test_function():
    """This has a teh typo in docstring."""
    # This comment has teh typo too
    x = 1  # Another teh here
    return x
'''

layer = SpellcheckLayer()
typos = layer._check_python_comments(python_code)

# Should find 3 typos (docstring + 2 comments)
assert len(typos) == 3
```

**Guidelines:**
- Use AST for docstring extraction (reliable)
- Handle syntax errors gracefully (skip file)
- Split on `#` to get comment portion
- Test with various comment styles

---

### 3.5 Spellcheck Layer - Wire Up Check Method ✅

**File:** `drep/documentation/spellcheck.py`

**COMPLETED:**
- [x] Implement `check(text, file_path)` method
- [x] Detect file type from `file_path` extension
- [x] Route to `_check_markdown()` for `.md` files
- [x] Route to `_check_python_comments()` for `.py` files
- [x] Route to `_check_plain_text()` for other files
- [x] Test with different file types
- [x] 3 tests passing

**Code to Update:**
```python
def check(self, text: str, file_path: str = "") -> List[Typo]:
    """Check text for typos with context awareness."""

    if file_path.endswith('.md'):
        return self._check_markdown(text)
    elif file_path.endswith('.py'):
        return self._check_python_comments(text)
    else:
        return self._check_plain_text(text)
```

**Testing:**
```python
layer = SpellcheckLayer()

# Test markdown
md_text = "This has teh typo"
typos = layer.check(md_text, "test.md")
assert len(typos) == 1

# Test Python
py_text = "# This has teh typo"
typos = layer.check(py_text, "test.py")
assert len(typos) == 1
```

**Guidelines:**
- Use file extension to determine file type
- Default to plain text if unknown extension
- Keep routing logic simple

---

### 3.6 Pattern Layer ✅

**File:** `drep/documentation/patterns.py`

**COMPLETED:**
- [x] Import regex, List
- [x] Import `PatternIssue` model
- [x] Create `PatternLayer` class
- [x] Define `PATTERNS` dict with regex patterns (double_space, trailing_whitespace)
- [x] Implement `check(text, file_ext)` method
- [x] For each pattern, find all matches using re.finditer
- [x] Calculate accurate line and column numbers for each match
- [x] Create `PatternIssue` for each match
- [x] Test with sample text
- [x] 5 tests passing

**Code Structure:**
```python
import re
from typing import List
from drep.models.findings import PatternIssue

class PatternLayer:
    """Layer 2: Pattern matching for common issues."""

    PATTERNS = {
        'double_space': (r'  +', ' '),
        'trailing_whitespace': (r' +$', ''),
    }

    def check(self, text: str, file_ext: str) -> List[PatternIssue]:
        """Check text for pattern issues."""
        issues = []

        for pattern_name, (regex, replacement) in self.PATTERNS.items():
            for match in re.finditer(regex, text, re.MULTILINE):
                # Find line number
                line_num = text[:match.start()].count('\n') + 1
                col_num = match.start() - text.rfind('\n', 0, match.start()) - 1

                issues.append(PatternIssue(
                    type=pattern_name,
                    line=line_num,
                    column=col_num,
                    matched_text=match.group(0),
                    replacement=replacement
                ))

        return issues
```

**Testing:**
```python
from drep.documentation.patterns import PatternLayer

layer = PatternLayer()

# Test double space
text = "This  has  double  spaces"
issues = layer.check(text, "md")
assert len(issues) == 3
assert all(i.type == 'double_space' for i in issues)

# Test trailing whitespace
text = "Line with trailing space   \n"
issues = layer.check(text, "md")
assert len(issues) == 1
assert issues[0].type == 'trailing_whitespace'
```

**Guidelines:**
- Keep patterns simple (start with 2-3)
- Use multiline mode for ^ and $ matching
- Calculate line/column carefully
- Test each pattern individually

---

### 3.7 Documentation Analyzer ✅

**File:** `drep/documentation/analyzer.py`

**COMPLETED:**
- [x] Import Path, SpellcheckLayer, PatternLayer
- [x] Import DocumentationConfig, DocumentationFindings
- [x] Create `DocumentationAnalyzer` class
- [x] Initialize with config and custom dictionary
- [x] Create instances of Layer 1 and Layer 2
- [x] Implement `analyze_file(file_path, content)` async method
- [x] Call Layer 1 and Layer 2 in sequence
- [x] Combine results into DocumentationFindings
- [x] Test end-to-end with Python and Markdown files
- [x] 5 tests passing

**Code Structure:**
```python
from pathlib import Path
from drep.models.config import DocumentationConfig
from drep.models.findings import DocumentationFindings
from drep.documentation.spellcheck import SpellcheckLayer
from drep.documentation.patterns import PatternLayer

class DocumentationAnalyzer:
    """Orchestrates tiered documentation analysis."""

    def __init__(self, config: DocumentationConfig):
        self.layer1 = SpellcheckLayer(
            custom_words=config.custom_dictionary
        )
        self.layer2 = PatternLayer()

    async def analyze_file(
        self,
        file_path: str,
        content: str
    ) -> DocumentationFindings:
        """Run tiered analysis on a file."""
        findings = DocumentationFindings(file_path=file_path)

        # Layer 1: Spellcheck
        typos = self.layer1.check(content, file_path=file_path)
        findings.typos = typos

        # Layer 2: Pattern matching
        file_ext = Path(file_path).suffix.lstrip('.')
        pattern_issues = self.layer2.check(content, file_ext)
        findings.pattern_issues = pattern_issues

        return findings
```

**Testing:**
```python
from drep.models.config import DocumentationConfig
from drep.documentation.analyzer import DocumentationAnalyzer
import asyncio

async def test():
    config = DocumentationConfig(
        enabled=True,
        custom_dictionary=["gitea"]
    )

    analyzer = DocumentationAnalyzer(config)

    # Test with sample Python file
    content = '''
def test():
    """This has teh typo."""
    pass  # trailing space
'''

    findings = await analyzer.analyze_file("test.py", content)

    print(f"Found {len(findings.typos)} typos")
    print(f"Found {len(findings.pattern_issues)} pattern issues")

asyncio.run(test())
```

**Guidelines:**
- Keep analyzer simple - just orchestration
- Let layers do the work
- Pass file_path to spellcheck for context
- Test with real files

---

### 3.8 Code Review Bug Fixes ✅

**Purpose:** Fix edge cases and bugs identified during thorough code review

**COMPLETED:**
- [x] Fixed 5 bugs total (4 Medium + 1 High severity)
- [x] All bugs have comprehensive test coverage
- [x] All 117 tests passing

#### Bug Fix 1: Module-level docstring line numbers (Medium)
**Issue:** Module docstrings (e.g., `"""Teh module."""` at top of file) were reported one line too far down. A typo on line 1 was reported as line 2.

**Root Cause:** Used fallback `definition_line=1` for `ast.Module` nodes, then added `typo.line`, resulting in 1+1=2.

**Fix:** For `ast.Module` nodes, use the docstring statement's actual line number (`node.body[0].lineno`) with proper offset calculation.

**Test:** `test_check_python_comments_module_docstring_line_number`

#### Bug Fix 2: Column tracking with inline code (Medium)
**Issue:** When the same word appears in inline code (`` `teh` ``) and in prose ("teh"), the column could point to the backtick instance instead of the prose instance.

**Root Cause:** Built occurrence index from cleaned line (backticks removed), then searched original line for Nth occurrence—mismatch between cleaned and original positions.

**Fix:** Complete refactor of `_check_line()` - iterate over original line, find backtick/URL spans first, skip words in those spans. Simpler and more accurate.

**Test:** `test_check_line_column_with_inline_code_and_prose`

#### Bug Fix 3: Comment column offset (Medium)
**Issue:** Typos in Python comments were reported with column positions relative to the comment text, not the original source line. For example: `return x  # This has teh typo` reported column ~10 instead of column 25.

**Root Cause:** Split line on `"#"` and only passed comment text to `_check_line()`, losing the original offset.

**Fix:** Track where the `"#"` appears in the original line and add `comment_start` offset to each typo's column after processing.

**Test:** `test_check_python_comments_inline_comment_column`

#### Bug Fix 4: Single-line docstrings on same line as def (Medium)
**Issue:** For uncommon but valid single-line docstrings like `def foo(): """Teh doc."""`, typos were reported on line 2 instead of line 1.

**Root Cause:** Used `node.lineno` (the def line) and then added `typo.line`, resulting in 1+1=2.

**Fix:** Use the docstring statement's own line number (`node.body[0].lineno`) for all cases, simplified offset calculation to `start_line + relative - 1`.

**Test:** `test_check_python_comments_single_line_docstring_same_line_as_def`

#### Bug Fix 5: Hash in string literals treated as comments (HIGH) ⚠️
**Issue:** Using simple string split (`line.split("#", 1)[1]`) to extract comments meant that ANY `#` in the source line was treated as a comment start, including `#` characters inside string literals. This caused three critical problems:
1. **FALSE POSITIVES:** Strings like `print("# teh string")` flagged "teh" as a comment typo when it's inside a string literal
2. **MISSED COMMENTS:** Lines like `url = "http://ex.com#anchor"  # Real comment` would extract the wrong text
3. **WRONG COLUMNS:** Column offsets calculated from the wrong `#` position

**Root Cause:** Naive string-based comment extraction without understanding Python syntax.

**Fix:** Complete refactor to use `tokenize.generate_tokens()` instead of string splitting. Use `tokenize.COMMENT` token type to identify real comments. Only true Python comments are now checked, strings are properly ignored.

**Tests:**
- `test_check_python_comments_ignores_hash_in_strings`
- `test_check_python_comments_string_then_real_comment`

**Impact:** This was the most critical bug as it could create incorrect issue reports in production, flagging valid code as having documentation typos.

---

## Phase 4: Repository Scanning

### 4.1 Repository Scanner - Basic Structure

**File:** `drep/core/scanner.py`

**TODO:**
- [ ] Import Repo from GitPython
- [ ] Import Path, List, Optional
- [ ] Import database models
- [ ] Create `RepositoryScanner` class with `__init__(db_session)`
- [ ] Create stub `scan_repository()` method
- [ ] Test instantiation

**Code Structure:**
```python
from git import Repo
from pathlib import Path
from typing import List, Optional
from drep.db.models import RepositoryScan

class RepositoryScanner:
    """Scans repositories with incremental diff support."""

    def __init__(self, db_session):
        self.db = db_session

    async def scan_repository(
        self,
        repo_path: str,
        owner: str,
        repo_name: str
    ) -> tuple[List[str], Optional[str]]:
        """Scan repository and return list of files + commit SHA."""
        # To be implemented
        return ([], None)
```

**Testing:**
```python
from drep.db import init_database
from drep.core.scanner import RepositoryScanner

db = init_database('sqlite:///test.db')
scanner = RepositoryScanner(db)
print("Scanner initialized")
```

**Guidelines:**
- Store db_session as instance variable
- Return tuple (files, sha) for flexibility

---

### 4.2 Repository Scanner - Get All Files

**File:** `drep/core/scanner.py`

**TODO:**
- [ ] Add `_get_all_python_files(repo_path)` method
- [ ] Use Path.glob() to find all `.py` and `.md` files
- [ ] Filter out ignored directories (venv, __pycache__, etc.)
- [ ] Return relative paths (not absolute)
- [ ] Test with real repository

**Code to Add:**
```python
def _get_all_python_files(self, repo_path: str) -> List[str]:
    """Get all Python and Markdown files in repository."""
    files = []
    repo_path = Path(repo_path)

    for pattern in ['**/*.py', '**/*.md']:
        files.extend([
            str(f.relative_to(repo_path))
            for f in repo_path.glob(pattern)
            if not self._should_ignore(f)
        ])

    return files

def _should_ignore(self, file_path: Path) -> bool:
    """Check if file should be ignored."""
    ignore_patterns = [
        '__pycache__',
        '.git',
        'venv',
        'env',
        '.venv',
        '.tox',
        'build',
        'dist',
    ]

    return any(pattern in str(file_path) for pattern in ignore_patterns)
```

**Testing:**
```python
scanner = RepositoryScanner(db)

# Test with drep repo itself
files = scanner._get_all_python_files('.')
print(f"Found {len(files)} files")
for f in files[:5]:
    print(f"  - {f}")

# Should NOT include files from venv/ or __pycache__/
assert not any('venv' in f for f in files)
assert not any('__pycache__' in f for f in files)
```

**Guidelines:**
- Use `**/*.py` for recursive glob
- Return relative paths (easier to work with)
- Extend ignore list as needed
- Test with actual repository

---

### 4.3 Repository Scanner - Get Changed Files

**File:** `drep/core/scanner.py`

**TODO:**
- [ ] Add `_get_changed_files(repo, old_sha, new_sha)` method
- [ ] Use GitPython to get diff between commits
- [ ] Extract file paths from diff items
- [ ] Filter to only `.py` and `.md` files
- [ ] Deduplicate (handle renames)
- [ ] Test with repository that has commits

**Code to Add:**
```python
def _get_changed_files(
    self,
    repo: Repo,
    old_sha: str,
    new_sha: str
) -> List[str]:
    """Get files changed between two commits."""
    diff = repo.commit(old_sha).diff(new_sha)

    changed_files = []
    for diff_item in diff:
        # Check both a_path and b_path (for renames)
        for path in [diff_item.a_path, diff_item.b_path]:
            if path and (path.endswith('.py') or path.endswith('.md')):
                changed_files.append(path)

    return list(set(changed_files))  # Deduplicate
```

**Testing:**
```python
# This requires a repo with at least 2 commits
from git import Repo

repo = Repo('.')
commits = list(repo.iter_commits(max_count=2))

if len(commits) >= 2:
    scanner = RepositoryScanner(db)
    changed = scanner._get_changed_files(
        repo,
        commits[1].hexsha,
        commits[0].hexsha
    )
    print(f"Changed files: {changed}")
```

**Guidelines:**
- Check both a_path and b_path (handles renames)
- Deduplicate with `set()`
- Filter to target file types only

---

### 4.4 Repository Scanner - Main Scan Logic

**File:** `drep/core/scanner.py`

**TODO:**
- [ ] Implement `scan_repository()` method
- [ ] Open repository with GitPython
- [ ] Get current commit SHA (handle empty repo)
- [ ] Query database for last scan
- [ ] If last scan exists, get changed files (incremental)
- [ ] If no last scan, get all files (full scan)
- [ ] Return files and SHA
- [ ] Test with empty repo
- [ ] Test with repo after first scan
- [ ] Test with repo after second scan (incremental)

**Code to Update:**
```python
async def scan_repository(
    self,
    repo_path: str,
    owner: str,
    repo_name: str
) -> tuple[List[str], Optional[str]]:
    """Scan repository and return list of files + commit SHA."""
    git_repo = Repo(repo_path)

    # Handle empty repos (no commits yet)
    try:
        current_sha = git_repo.head.commit.hexsha
    except ValueError:
        # Repo has no commits yet
        return ([], None)

    # Get last scan
    last_scan = self.db.query(RepositoryScan).filter_by(
        owner=owner,
        repo=repo_name
    ).order_by(RepositoryScan.scanned_at.desc()).first()

    if last_scan:
        # Incremental scan - only changed files
        files = self._get_changed_files(
            git_repo,
            last_scan.commit_sha,
            current_sha
        )
    else:
        # Full scan - all Python files
        files = self._get_all_python_files(repo_path)

    return (files, current_sha)

def record_scan(self, owner: str, repo_name: str, commit_sha: str):
    """Record successful scan in database."""
    new_scan = RepositoryScan(
        owner=owner,
        repo=repo_name,
        commit_sha=commit_sha
    )
    self.db.add(new_scan)
    self.db.commit()
```

**Testing:**
```python
import asyncio

async def test():
    db = init_database('sqlite:///test.db')
    scanner = RepositoryScanner(db)

    # First scan (should get all files)
    files, sha = await scanner.scan_repository('.', 'steve', 'drep')
    print(f"First scan: {len(files)} files, SHA: {sha[:8]}")

    # Record scan
    scanner.record_scan('steve', 'drep', sha)

    # Second scan (should get no files if no changes)
    files2, sha2 = await scanner.scan_repository('.', 'steve', 'drep')
    print(f"Second scan: {len(files2)} files")

asyncio.run(test())
```

**Guidelines:**
- Always handle empty repos (ValueError)
- Query for last scan before deciding full vs incremental
- Return SHA even if no files (needed for recording)
- Test incremental scanning thoroughly

---

## Phase 5: Issue Management

### 5.1 Issue Manager - Basic Structure

**File:** `drep/core/issue_manager.py`

**TODO:**
- [ ] Import hashlib, List
- [ ] Import Finding model, FindingCache
- [ ] Create `IssueManager` class with `__init__(adapter, db_session)`
- [ ] Store adapter and db as instance variables
- [ ] Create stub `create_issues_for_findings()` method

**Code Structure:**
```python
import hashlib
from typing import List
from drep.models.findings import Finding
from drep.db.models import FindingCache

class IssueManager:
    """Manages issue creation with deduplication."""

    def __init__(self, adapter, db_session):
        self.adapter = adapter
        self.db = db_session

    async def create_issues_for_findings(
        self,
        owner: str,
        repo: str,
        findings: List[Finding]
    ):
        """Create issues for findings, skipping duplicates."""
        # To be implemented
        pass
```

**Testing:**
```python
from drep.core.issue_manager import IssueManager

# Need adapter and db
adapter = GiteaAdapter("http://192.168.1.14:3000", "token")
db = init_database('sqlite:///test.db')
manager = IssueManager(adapter, db)
print("Issue manager initialized")
```

**Guidelines:**
- Store both adapter and db (needed for operations)
- Keep interface simple (list of findings in, issues created)

---

### 5.2 Issue Manager - Generate Hash

**File:** `drep/core/issue_manager.py`

**TODO:**
- [ ] Add `_generate_hash(finding)` method
- [ ] Create string from file_path, line, type, message
- [ ] Hash with MD5
- [ ] Return hex digest
- [ ] Test that same finding produces same hash

**Code to Add:**
```python
def _generate_hash(self, finding: Finding) -> str:
    """Generate unique hash for finding."""
    content = f"{finding.file_path}:{finding.line}:{finding.type}:{finding.message}"
    return hashlib.md5(content.encode()).hexdigest()
```

**Testing:**
```python
from drep.models.findings import Finding

manager = IssueManager(adapter, db)

finding1 = Finding(
    type='typo',
    severity='info',
    file_path='test.py',
    line=5,
    message="Typo: 'teh'"
)

finding2 = Finding(
    type='typo',
    severity='info',
    file_path='test.py',
    line=5,
    message="Typo: 'teh'"
)

hash1 = manager._generate_hash(finding1)
hash2 = manager._generate_hash(finding2)

assert hash1 == hash2  # Same finding, same hash
```

**Guidelines:**
- Include all identifying info in hash (file, line, type, message)
- Use MD5 (fast, collision unlikely for this use case)
- Test hash consistency

---

### 5.3 Issue Manager - Generate Issue Body

**File:** `drep/core/issue_manager.py`

**TODO:**
- [ ] Add `_generate_issue_body(finding)` method
- [ ] Create markdown formatted body
- [ ] Include type, severity, file, line
- [ ] Include message and suggestion
- [ ] Add footer with drep attribution
- [ ] Test markdown formatting

**Code to Add:**
```python
def _generate_issue_body(self, finding: Finding) -> str:
    """Generate issue body."""
    body = f"""## Finding

**Type:** {finding.type}
**Severity:** {finding.severity}
**File:** {finding.file_path}
**Line:** {finding.line}

**Issue:** {finding.message}
"""

    if finding.suggestion:
        body += f"\n**Suggestion:** {finding.suggestion}\n"

    body += "\n---\n*Automatically created by [drep](https://github.com/stephenbrandon/drep)*"

    return body
```

**Testing:**
```python
finding = Finding(
    type='typo',
    severity='info',
    file_path='test.py',
    line=5,
    message="Typo: 'teh'",
    suggestion="Did you mean 'the'?"
)

body = manager._generate_issue_body(finding)
print(body)

# Verify markdown formatting
assert '**Type:** typo' in body
assert '**Suggestion:** ' in body
```

**Guidelines:**
- Use markdown for formatting
- Include all relevant info
- Keep formatting consistent
- Test that output looks good in Gitea

---

### 5.4 Issue Manager - Create Issues

**File:** `drep/core/issue_manager.py`

**TODO:**
- [ ] Implement `create_issues_for_findings()` method
- [ ] Loop through each finding
- [ ] Generate hash for finding
- [ ] Check if hash exists in FindingCache
- [ ] If exists, skip (already created issue)
- [ ] If not exists, create issue via adapter
- [ ] Store in FindingCache with issue number
- [ ] Commit to database
- [ ] Test with multiple findings
- [ ] Test deduplication

**Code to Update:**
```python
async def create_issues_for_findings(
    self,
    owner: str,
    repo: str,
    findings: List[Finding]
):
    """Create issues for findings, skipping duplicates."""

    for finding in findings:
        # Generate hash for deduplication
        finding_hash = self._generate_hash(finding)

        # Check if we've already created this issue
        existing = self.db.query(FindingCache).filter_by(
            finding_hash=finding_hash
        ).first()

        if existing:
            continue  # Skip duplicate

        # Create issue
        title = f"[drep] {finding.type}: {finding.file_path}:{finding.line}"
        body = self._generate_issue_body(finding)

        issue_number = await self.adapter.create_issue(
            owner=owner,
            repo=repo,
            title=title,
            body=body,
            labels=['documentation', 'automated']
        )

        # Cache this finding
        cache_entry = FindingCache(
            owner=owner,
            repo=repo,
            file_path=finding.file_path,
            finding_hash=finding_hash,
            issue_number=issue_number
        )
        self.db.add(cache_entry)
        self.db.commit()
```

**Testing:**
```python
import asyncio

async def test():
    adapter = GiteaAdapter("http://192.168.1.14:3000", "YOUR_TOKEN")
    db = init_database('sqlite:///test.db')
    manager = IssueManager(adapter, db)

    findings = [
        Finding(
            type='typo',
            severity='info',
            file_path='test.py',
            line=5,
            message="Typo: 'teh'",
            suggestion="Did you mean 'the'?"
        )
    ]

    # First call - should create issue
    await manager.create_issues_for_findings('steve', 'test-repo', findings)

    # Second call with same finding - should skip (deduplication)
    await manager.create_issues_for_findings('steve', 'test-repo', findings)

    await adapter.close()

asyncio.run(test())
# Should create 1 issue, not 2
```

**Guidelines:**
- Always check cache before creating issue
- Store issue_number in cache (for future reference)
- Commit after each issue (atomic operations)
- Test deduplication thoroughly

---

## Phase 6: CLI & Integration

### 6.1 CLI - Basic Structure

**File:** `drep/cli.py`

**TODO:**
- [ ] Import Click, asyncio, Path
- [ ] Create `cli()` group function
- [ ] Add docstring
- [ ] Test `drep --help`

**Code Structure:**
```python
import click
import asyncio
from pathlib import Path

@click.group()
def cli():
    """drep - Documentation & Review Enhancement Platform"""
    pass

if __name__ == '__main__':
    cli()
```

**Testing:**
```bash
# Install in development mode first
pip install -e .

# Test help
drep --help
# Should show: drep - Documentation & Review Enhancement Platform
```

**Guidelines:**
- Use `@click.group()` for multi-command CLI
- Keep help text concise
- Test that CLI is accessible after install

---

### 6.2 CLI - Init Command

**File:** `drep/cli.py`

**TODO:**
- [ ] Add `init()` command with `@cli.command()`
- [ ] Check if config.yaml already exists
- [ ] If exists, ask for confirmation to overwrite
- [ ] Create example config.yaml with template
- [ ] Use environment variable placeholders
- [ ] Print success message
- [ ] Test command

**Code to Add:**
```python
@cli.command()
def init():
    """Initialize drep configuration."""
    config_path = Path('config.yaml')

    if config_path.exists():
        click.confirm('config.yaml already exists. Overwrite?', abort=True)

    # Create example config
    example = """gitea:
  url: http://192.168.1.14:3000
  token: ${GITEA_TOKEN}
  repositories:
    - steve/*

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - fastapi
    - gitea

database_url: sqlite:///./drep.db
"""

    config_path.write_text(example)
    click.echo(f"✓ Created {config_path}")
    click.echo("\nEdit config.yaml to add your Gitea token.")
```

**Testing:**
```bash
# Run init command
drep init

# Should create config.yaml
ls -la config.yaml

# Run again - should prompt for overwrite
drep init
# Type 'n' to abort

# Check content
cat config.yaml
```

**Guidelines:**
- Always check for existing file
- Use `click.confirm()` for destructive operations
- Provide clear next steps in output
- Use environment variable placeholders

---

### 6.3 CLI - Scan Command (Basic)

**File:** `drep/cli.py`

**TODO:**
- [ ] Add `scan()` command
- [ ] Add `repository` argument (owner/repo format)
- [ ] Add `--config` option (default: config.yaml)
- [ ] Parse owner/repo from argument
- [ ] Call `_run_scan()` helper with asyncio.run()
- [ ] Test with repository argument

**Code to Add:**
```python
@cli.command()
@click.argument('repository')
@click.option('--config', default='config.yaml', help='Config file path')
def scan(repository, config):
    """Scan a repository: drep scan owner/repo"""

    if '/' not in repository:
        click.echo("Error: Repository must be in format 'owner/repo'", err=True)
        return

    owner, repo_name = repository.split('/', 1)

    click.echo(f"Scanning {owner}/{repo_name}...")

    # Run async scan
    asyncio.run(_run_scan(owner, repo_name, config))

    click.echo("✓ Scan complete")

async def _run_scan(owner: str, repo: str, config_path: str):
    """Run the actual scan."""
    # To be implemented
    pass
```

**Testing:**
```bash
drep scan steve/drep --config config.yaml
# Should print: Scanning steve/drep...
```

**Guidelines:**
- Validate repository format
- Use asyncio.run() for async main function
- Keep sync/async separation clean

---

### 6.4 CLI - Scan Implementation (Git Operations)

**File:** `drep/cli.py`

**TODO:**
- [ ] Load config in `_run_scan()`
- [ ] Initialize all components (adapter, db, scanner, analyzer, issue manager)
- [ ] Create temp directory and askpass script
- [ ] Build git environment variables
- [ ] Check if repo exists locally
- [ ] If not, clone it (with authentication)
- [ ] If exists, pull latest
- [ ] Clean up temp directory in finally block
- [ ] Test clone operation
- [ ] Test pull operation

**Code to Add:**
```python
from drep.config import load_config
from drep.adapters.gitea import GiteaAdapter
from drep.db import init_database
from drep.core.scanner import RepositoryScanner
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.core.issue_manager import IssueManager
from git import Git, Repo
import os
import tempfile
import shutil

async def _run_scan(owner: str, repo: str, config_path: str):
    """Run the actual scan."""
    # Load config
    config = load_config(config_path)

    # Initialize components
    adapter = GiteaAdapter(config.gitea.url, config.gitea.token)
    db_session = init_database(config.database_url)
    scanner = RepositoryScanner(db_session)
    analyzer = DocumentationAnalyzer(config.documentation)
    issue_manager = IssueManager(adapter, db_session)

    try:
        # Clone/fetch repository
        repo_path = f"./repos/{owner}/{repo}"

        # Create temporary askpass script
        temp_dir = tempfile.mkdtemp(prefix='drep_')
        askpass_script = Path(temp_dir) / 'askpass.sh'

        askpass_script_content = '''#!/bin/sh
if echo "$1" | grep -qi "username"; then
    # Return "token" as username (Gitea expects non-empty username)
    echo "token"
elif echo "$1" | grep -qi "password"; then
    echo "$DREP_GIT_TOKEN"
else
    echo "$DREP_GIT_TOKEN"
fi
'''
        askpass_script.write_text(askpass_script_content)
        askpass_script.chmod(0o700)

        # Set up git environment
        git_env = {
            **os.environ,
            'GIT_ASKPASS': str(askpass_script),
            'GIT_TERMINAL_PROMPT': '0',
            'DREP_GIT_TOKEN': config.gitea.token,
        }

        try:
            if not Path(repo_path).exists():
                # Create parent directory
                Path(repo_path).parent.mkdir(parents=True, exist_ok=True)

                # Get default branch
                default_branch = await adapter.get_default_branch(owner, repo)

                # Clone with clean URL
                base_url = config.gitea.url.rstrip('/')
                clean_git_url = f"{base_url}/{owner}/{repo}.git"

                # Clone with environment variables
                # Pass env directly to clone_from (custom_environment doesn't work here)
                git_repo = Repo.clone_from(
                    clean_git_url,
                    repo_path,
                    branch=default_branch,
                    env=git_env
                )
            else:
                # Pull latest
                git_repo = Repo(repo_path)
                with git_repo.git.custom_environment(**git_env):
                    git_repo.remotes.origin.pull()

        finally:
            # Clean up temp script
            shutil.rmtree(temp_dir, ignore_errors=True)

        # TODO: Scan and analyze files

    finally:
        # Always close HTTP client
        await adapter.close()
```

**Testing:**
```bash
# Set token
export GITEA_TOKEN="your_token_here"

# Run scan
drep scan steve/drep

# Check that repos/steve/drep was created
ls -la repos/steve/drep
```

**Guidelines:**
- Create temp directory for askpass script
- Always clean up temp files
- Use custom_environment for all git operations
- Test with both public and private repos

---

### 6.5 CLI - Scan Implementation (Analysis)

**File:** `drep/cli.py`

**TODO:**
- [ ] Add scanning logic after git operations
- [ ] Call `scanner.scan_repository()` to get files
- [ ] Handle empty repo case (no commits)
- [ ] Handle no files case
- [ ] Loop through each file
- [ ] Read file content
- [ ] Call `analyzer.analyze_file()`
- [ ] Convert to findings
- [ ] Collect all findings
- [ ] Test analysis

**Code to Add (in `_run_scan` after git operations):**
```python
        # Scan repository
        files_to_analyze, current_sha = await scanner.scan_repository(repo_path, owner, repo)

        if current_sha is None:
            click.echo("Repository has no commits yet. Skipping.")
            return

        click.echo(f"Analyzing {len(files_to_analyze)} files...")

        # Analyze files
        all_findings = []
        if files_to_analyze:
            for file_path in files_to_analyze:
                full_path = Path(repo_path) / file_path
                content = full_path.read_text()

                findings_obj = await analyzer.analyze_file(str(file_path), content)
                all_findings.extend(findings_obj.to_findings())

            click.echo(f"Found {len(all_findings)} issues")
        else:
            click.echo("No Python/Markdown files to analyze.")

        # TODO: Create issues
```

**Testing:**
```bash
drep scan steve/drep

# Should print:
# Analyzing N files...
# Found M issues
```

**Guidelines:**
- Always handle empty repo case
- Print progress for user feedback
- Read files with proper encoding
- Test with repo that has typos

---

### 6.6 CLI - Scan Implementation (Issue Creation)

**File:** `drep/cli.py`

**TODO:**
- [ ] Add issue creation after analysis
- [ ] Call `issue_manager.create_issues_for_findings()`
- [ ] Record scan in database after success
- [ ] Test end-to-end: scan → analyze → create issues
- [ ] Verify issues appear in Gitea

**Code to Add (in `_run_scan` after analysis):**
```python
        # Create issues
        if all_findings:
            await issue_manager.create_issues_for_findings(owner, repo, all_findings)

        # Record scan in DB after successful completion
        scanner.record_scan(owner, repo, current_sha)
```

**Testing:**
```bash
# Full end-to-end test
export GITEA_TOKEN="your_token"
drep scan steve/test-repo

# Check Gitea UI for created issues
# Should see issues with [drep] prefix
```

**Guidelines:**
- Only create issues if findings exist
- Only record scan after successful completion
- Test complete workflow
- Verify issues in Gitea UI

---

## Phase 7: Testing & Polish

### 7.1 Unit Tests - Spellcheck

**File:** `tests/unit/test_spellcheck.py`

**TODO:**
- [ ] Create test file
- [ ] Import pytest, SpellcheckLayer
- [ ] Test `_check_line()` with typos
- [ ] Test `_check_line()` with camelCase (should skip)
- [ ] Test `_check_line()` with URLs (should skip)
- [ ] Test `_check_markdown()` with code blocks
- [ ] Test `_check_python_comments()` with comments and docstrings
- [ ] Test custom dictionary
- [ ] Run tests with pytest

**Code Structure:**
```python
import pytest
from drep.documentation.spellcheck import SpellcheckLayer

def test_check_line_finds_typo():
    layer = SpellcheckLayer()
    typos = layer._check_line("This has teh typo", 1)

    assert len(typos) == 1
    assert typos[0].word == "teh"
    assert "the" in typos[0].suggestions

def test_check_line_ignores_camelcase():
    layer = SpellcheckLayer()
    typos = layer._check_line("The myVariable is camelCase", 1)

    assert len(typos) == 0

def test_check_line_ignores_urls():
    layer = SpellcheckLayer()
    typos = layer._check_line("Check https://example.com", 1)

    assert len(typos) == 0

def test_check_markdown_skips_code_blocks():
    layer = SpellcheckLayer()
    text = """
This has teh typo.

```
This has teh in code block
```

Another teh typo.
"""
    typos = layer._check_markdown(text)

    # Should find 2 typos (not the one in code block)
    assert len(typos) == 2

def test_custom_dictionary():
    layer = SpellcheckLayer(custom_words=["gitea", "drep"])
    typos = layer._check_line("gitea and drep are great", 1)

    # Should not flag gitea or drep as typos
    assert len(typos) == 0
```

**Running:**
```bash
pytest tests/unit/test_spellcheck.py -v
```

**Guidelines:**
- Test one function at a time
- Test both positive and negative cases
- Test edge cases (URLs, code, etc.)
- Use descriptive test names

---

### 7.2 Unit Tests - Pattern Layer

**File:** `tests/unit/test_patterns.py`

**TODO:**
- [ ] Create test file
- [ ] Test double space detection
- [ ] Test trailing whitespace detection
- [ ] Test line/column calculation
- [ ] Run tests

**Code Structure:**
```python
import pytest
from drep.documentation.patterns import PatternLayer

def test_double_space_detection():
    layer = PatternLayer()
    text = "This  has  double  spaces"
    issues = layer.check(text, "md")

    assert len(issues) == 3
    assert all(i.type == 'double_space' for i in issues)

def test_trailing_whitespace():
    layer = PatternLayer()
    text = "Line with trailing   \n"
    issues = layer.check(text, "md")

    assert len(issues) == 1
    assert issues[0].type == 'trailing_whitespace'
    assert issues[0].matched_text == '   '

def test_line_column_calculation():
    layer = PatternLayer()
    text = "Line 1\nLine  2 with double\nLine 3"
    issues = layer.check(text, "md")

    assert len(issues) == 1
    assert issues[0].line == 2  # Second line
```

**Running:**
```bash
pytest tests/unit/test_patterns.py -v
```

**Guidelines:**
- Test each pattern type
- Verify line/column numbers are correct
- Test multiline text

---

### 7.3 Integration Test - Full Scan

**File:** `tests/integration/test_full_scan.py`

**TODO:**
- [ ] Create test repository with known typos
- [ ] Run full scan programmatically
- [ ] Verify findings are correct
- [ ] Verify issues are created
- [ ] Clean up test data

**Code Structure:**
```python
import pytest
import asyncio
from pathlib import Path
from drep.config import load_config
from drep.adapters.gitea import GiteaAdapter
from drep.db import init_database
from drep.core.scanner import RepositoryScanner
from drep.documentation.analyzer import DocumentationAnalyzer
from drep.core.issue_manager import IssueManager

@pytest.mark.asyncio
async def test_full_scan_workflow():
    """Integration test for complete scan workflow."""

    # Load config (use test config)
    config = load_config('test_config.yaml')

    # Initialize components
    adapter = GiteaAdapter(config.gitea.url, config.gitea.token)
    db = init_database('sqlite:///test_integration.db')
    scanner = RepositoryScanner(db)
    analyzer = DocumentationAnalyzer(config.documentation)
    issue_manager = IssueManager(adapter, db)

    try:
        # Scan test repository
        repo_path = './tests/fixtures/test_repo'
        files, sha = await scanner.scan_repository(repo_path, 'test', 'repo')

        assert sha is not None
        assert len(files) > 0

        # Analyze files
        all_findings = []
        for file_path in files:
            full_path = Path(repo_path) / file_path
            content = full_path.read_text()

            findings_obj = await analyzer.analyze_file(str(file_path), content)
            all_findings.extend(findings_obj.to_findings())

        # Should find at least some issues
        assert len(all_findings) > 0

        # Test findings have correct structure
        for finding in all_findings:
            assert finding.file_path
            assert finding.line > 0
            assert finding.message

    finally:
        await adapter.close()
```

**Setup:**
```bash
# Create test fixture repository
mkdir -p tests/fixtures/test_repo
cd tests/fixtures/test_repo
git init
echo "# Test\nThis has teh typo" > README.md
git add README.md
git commit -m "Initial commit"
cd -

# Run integration test
pytest tests/integration/test_full_scan.py -v
```

**Guidelines:**
- Use fixtures for test data
- Clean up after tests
- Test realistic scenarios
- Verify all components work together

---

### 7.4 Manual Testing Checklist

**TODO:**
- [ ] Test `drep init` creates config.yaml
- [ ] Test `drep scan` with non-existent repo (should error gracefully)
- [ ] Test `drep scan` with empty repo (should skip)
- [ ] Test `drep scan` with repo containing typos (should find them)
- [ ] Test `drep scan` twice (second should be incremental)
- [ ] Test with invalid Gitea token (should error)
- [ ] Test with private repository (should authenticate)
- [ ] Verify issues appear in Gitea UI
- [ ] Verify issue format is correct (markdown, labels)
- [ ] Test deduplication (run scan twice, should not create duplicate issues)

**Commands:**
```bash
# Test init
drep init
cat config.yaml

# Test with non-existent repo
drep scan fake/nonexistent
# Should show error

# Test with real repo
export GITEA_TOKEN="your_token"
drep scan steve/drep

# Check Gitea for issues
# Visit http://192.168.1.14:3000/steve/drep/issues

# Run again (test incremental + deduplication)
drep scan steve/drep
# Should skip files (unless changed) and not create duplicate issues
```

**Guidelines:**
- Test all error paths
- Test with real Gitea instance
- Verify output in UI
- Document any bugs found

---

### 7.5 Documentation

**TODO:**
- [ ] Review README.md for accuracy
- [ ] Add troubleshooting section to README
- [ ] Add examples to README
- [ ] Update CHANGELOG.md with MVP completion
- [ ] Add inline code comments where needed
- [ ] Add docstrings to all public methods

**Troubleshooting Section (add to README):**
```markdown
## Troubleshooting

### "Repository not found" error
- Verify repository exists in Gitea
- Check that owner/repo format is correct
- Ensure your token has access to the repository

### "Unauthorized" error
- Check that GITEA_TOKEN environment variable is set
- Verify token has correct permissions in Gitea
- Try generating a new token

### Clone fails for private repository
- Ensure token is passed correctly
- Check network connectivity to Gitea server
- Verify SSH/HTTP access is configured in Gitea

### No issues created despite findings
- Check database (drep.db) for FindingCache entries
- Verify token has permission to create issues
- Check Gitea issue creation settings
```

**Guidelines:**
- Keep docs up to date with code
- Add examples for common use cases
- Document all error messages
- Include screenshots if helpful

---

## Success Criteria

The MVP is complete when ALL of the following are true:

### Functionality
- [ ] `drep init` creates valid config.yaml
- [ ] `drep scan owner/repo` successfully scans Gitea repository
- [ ] Typos in Python comments/docstrings are detected
- [ ] Typos in markdown files are detected
- [ ] Code blocks in markdown are skipped (no false positives)
- [ ] Code identifiers (camelCase, etc.) are not flagged as typos
- [ ] Pattern issues (double space, trailing whitespace) are detected
- [ ] Issues are created on Gitea with proper formatting
- [ ] Issues have correct labels (documentation, automated)
- [ ] Duplicate issues are not created (deduplication works)
- [ ] Incremental scanning works (only scans changed files)
- [ ] Empty repositories are handled gracefully
- [ ] Private repositories can be scanned (authentication works)

### Code Quality
- [ ] All functions have docstrings
- [ ] Code follows consistent style (use black/ruff)
- [ ] No hardcoded credentials
- [ ] Error messages are helpful
- [ ] Logging is appropriate (not too verbose, not too quiet)

### Testing
- [ ] Unit tests pass for spellcheck layer
- [ ] Unit tests pass for pattern layer
- [ ] Integration test passes for full scan workflow
- [ ] Manual testing checklist completed
- [ ] No known critical bugs

### Documentation
- [ ] README is accurate and complete
- [ ] CHANGELOG reflects MVP completion
- [ ] Troubleshooting section exists
- [ ] Code comments explain complex logic

### Performance
- [ ] First scan completes in < 60 seconds for small repos (< 100 files)
- [ ] Incremental scan completes in < 30 seconds
- [ ] No memory leaks (test with multiple scans)

---

## Common Pitfalls to Avoid

### 1. Environment Variables
**Pitfall:** Forgetting to set GITEA_TOKEN
**Solution:** Always check if env var is set, provide clear error message

### 2. Git Authentication
**Pitfall:** Token in git URL (security issue)
**Solution:** Use GIT_ASKPASS with temp script and env var

### 3. Database Session Management
**Pitfall:** Not committing transactions
**Solution:** Always call `db.commit()` after db.add()

### 4. File Encoding
**Pitfall:** Assuming UTF-8 encoding
**Solution:** Use `encoding='utf-8'` when reading files, handle errors

### 5. Empty Repositories
**Pitfall:** Crashing on repos with no commits
**Solution:** Catch ValueError from git_repo.head.commit

### 6. Incremental Scanning
**Pitfall:** Recording scan before completion (data loss on crash)
**Solution:** Only call `record_scan()` after successful completion

### 7. HTTP Client Cleanup
**Pitfall:** Not closing httpx.AsyncClient (resource leak)
**Solution:** Always use try/finally and call adapter.close()

### 8. Temp File Cleanup
**Pitfall:** Leaving askpass script in /tmp
**Solution:** Use finally block to remove temp directory

---

## Next Steps After MVP

Once MVP is complete, consider these enhancements (in order):

### Phase 2: Intelligence
1. Integrate LLM via open-agent-sdk (Layer 3)
2. Add missing comment detection
3. Add bad comment detection
4. Generate comment suggestions

### Phase 3: Automation
1. Add Gitea webhook support
2. Add FastAPI server
3. Implement draft PR creation
4. Add automatic scanning on push

### Phase 4: Language Expansion
1. Add JavaScript/TypeScript support
2. Add JSDoc parsing
3. Improve spellcheck accuracy

### Phase 5: Multi-Platform
1. Add GitHub adapter
2. Add GitLab adapter
3. Cross-platform testing

---

**End of Implementation Plan**
