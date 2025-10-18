# Technical Design: drep

**Document Version:** 2.0
**Last Updated:** 2025-10-17
**Status:** Ready for Implementation

---

## Executive Summary

**drep** (Documentation & Review Enhancement Platform) is an automated code review tool for **Gitea** that detects and fixes documentation issues in **Python** repositories.

### MVP Scope

- **Platform:** Gitea only (self-hosted at 192.168.1.14)
- **Language:** Python only
- **Features:**
  - Typo detection in comments, docstrings, and markdown
  - Pattern matching for formatting issues
  - Issue creation on Gitea with findings
  - Incremental scanning (only changed files)
- **Interface:** CLI command `drep scan owner/repo`

### Post-MVP Expansions

- **Phase 2:** LLM integration, draft PRs, webhooks
- **Phase 3:** JavaScript/TypeScript support
- **Phase 4:** GitHub/GitLab adapters
- **Phase 5:** Vector database, additional languages

---

## System Architecture

### MVP Architecture

```
┌─────────────────────────────────────────┐
│            Gitea Server                 │
│        (192.168.1.14:3000)             │
└──────────────┬──────────────────────────┘
               │ API Calls
               ▼
┌──────────────────────────────────────────┐
│         drep CLI                         │
│                                          │
│  ┌────────────────────────────┐         │
│  │  Gitea Adapter             │         │
│  │  - get_file_content()      │         │
│  │  - create_issue()          │         │
│  └────────────────────────────┘         │
│                                          │
│  ┌────────────────────────────┐         │
│  │  Documentation Analyzer    │         │
│  │  ┌──────────────────────┐  │         │
│  │  │ Layer 1: Spellcheck  │  │         │
│  │  └──────────────────────┘  │         │
│  │  ┌──────────────────────┐  │         │
│  │  │ Layer 2: Patterns    │  │         │
│  │  └──────────────────────┘  │         │
│  └────────────────────────────┘         │
│                                          │
│  ┌────────────────────────────┐         │
│  │  Repository Scanner        │         │
│  │  - Incremental diff scan   │         │
│  │  - Python file extraction  │         │
│  └────────────────────────────┘         │
│                                          │
│  ┌────────────────────────────┐         │
│  │  SQLite Database           │         │
│  │  - Last scan tracking      │         │
│  │  - Finding deduplication   │         │
│  └────────────────────────────┘         │
└──────────────────────────────────────────┘
```

---

## Technology Stack

```toml
[project]
dependencies = [
    "click>=8.1.0",              # CLI framework
    "httpx>=0.25.0",             # HTTP client for Gitea API
    "gitpython>=3.1.0",          # Git operations
    "pyspellchecker>=0.7.0",     # Spellcheck (Layer 1)
    "pydantic>=2.5.0",           # Data validation
    "sqlalchemy>=2.0.0",         # ORM
    "aiosqlite>=0.19.0",         # Async SQLite
    "pyyaml>=6.0",               # Config parsing
]
```

**Not in MVP:** FastAPI, uvicorn, open-agent-sdk (added in Phase 2)

---

## Data Models

### Configuration

```python
from pydantic import BaseModel, Field
from typing import List

class GiteaConfig(BaseModel):
    """Gitea platform configuration."""
    url: str = Field(..., description="Gitea base URL")
    token: str = Field(..., description="API token")
    repositories: List[str] = Field(..., description="Repo patterns to monitor")

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

### Findings

```python
from pydantic import BaseModel
from typing import Optional, List

class Typo(BaseModel):
    """Typo with explicit fields for safe fixing."""
    word: str                    # The misspelled word
    replacement: str             # The correct spelling
    line: int
    column: int
    context: str                 # Surrounding text
    suggestions: List[str] = []  # Alternative corrections

class PatternIssue(BaseModel):
    """Pattern matching issue."""
    type: str                    # 'double_space', 'trailing_whitespace', etc.
    line: int
    column: int
    matched_text: str
    replacement: str

class Finding(BaseModel):
    """Generic finding for issue creation."""
    type: str                    # 'typo', 'pattern'
    severity: str                # 'info', 'warning', 'error'
    file_path: str
    line: int
    column: Optional[int] = None

    # Explicit fields for safe fixing (Phase 2)
    original: Optional[str] = None
    replacement: Optional[str] = None

    # Human-readable
    message: str
    suggestion: Optional[str] = None

class DocumentationFindings(BaseModel):
    """Results from documentation analysis."""
    file_path: str
    typos: List[Typo] = []
    pattern_issues: List[PatternIssue] = []

    def to_findings(self) -> List[Finding]:
        """Convert to generic Finding objects."""
        findings = []

        for typo in self.typos:
            findings.append(Finding(
                type='typo',
                severity='info',
                file_path=self.file_path,
                line=typo.line,
                column=typo.column,
                original=typo.word,
                replacement=typo.replacement,
                message=f"Typo: '{typo.word}'",
                suggestion=f"Did you mean '{typo.replacement}'?"
            ))

        for issue in self.pattern_issues:
            findings.append(Finding(
                type='pattern',
                severity='info',
                file_path=self.file_path,
                line=issue.line,
                column=issue.column,
                original=issue.matched_text,
                replacement=issue.replacement,
                message=f"Pattern issue: {issue.type}",
                suggestion=f"Replace with: {issue.replacement}"
            ))

        return findings
```

### Database Models

```python
from sqlalchemy import Column, Integer, String, DateTime, JSON
from sqlalchemy.ext.declarative import declarative_base
from datetime import datetime

Base = declarative_base()

class RepositoryScan(Base):
    """Tracks last scan for incremental scanning."""
    __tablename__ = 'repository_scans'

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    commit_sha = Column(String, nullable=False)
    scanned_at = Column(DateTime, default=datetime.utcnow)

class FindingCache(Base):
    """Prevents duplicate issues."""
    __tablename__ = 'finding_cache'

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    file_path = Column(String, nullable=False)
    finding_hash = Column(String, nullable=False, unique=True)
    issue_number = Column(Integer, nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)
```

---

## Core Components

### 1. Gitea Adapter

```python
import httpx
from typing import List, Optional

class GiteaAdapter:
    """Gitea API adapter."""

    def __init__(self, url: str, token: str):
        self.url = url.rstrip('/')
        self.token = token
        self.client = httpx.AsyncClient(
            headers={'Authorization': f'token {token}'}
        )

    async def get_file_content(
        self,
        owner: str,
        repo: str,
        path: str,
        ref: str = "main"
    ) -> str:
        """Get file content from repository."""
        url = f"{self.url}/api/v1/repos/{owner}/{repo}/contents/{path}"
        response = await self.client.get(url, params={'ref': ref})
        response.raise_for_status()

        data = response.json()
        # Gitea returns base64-encoded content
        import base64
        return base64.b64decode(data['content']).decode('utf-8')

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

        response = await self.client.post(url, json=payload)
        response.raise_for_status()

        return response.json()['number']

    async def get_default_branch(self, owner: str, repo: str) -> str:
        """Get repository default branch."""
        url = f"{self.url}/api/v1/repos/{owner}/{repo}"
        response = await self.client.get(url)
        response.raise_for_status()

        return response.json()['default_branch']

    async def close(self):
        """Close HTTP client connection."""
        await self.client.aclose()
```

---

### 2. Documentation Analyzer

```python
from pathlib import Path

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

        # Layer 1: Spellcheck (PASS file_path for context)
        typos = self.layer1.check(content, file_path=file_path)
        findings.typos = typos

        # Layer 2: Pattern matching
        file_ext = Path(file_path).suffix.lstrip('.')
        pattern_issues = self.layer2.check(content, file_ext)
        findings.pattern_issues = pattern_issues

        return findings
```

---

### 3. Spellcheck Layer (Context-Aware)

```python
import re
from spellchecker import SpellChecker
from pathlib import Path

class SpellcheckLayer:
    """Layer 1: Dictionary spellcheck with context awareness."""

    def __init__(self, custom_words: List[str] = None):
        self.spell = SpellChecker()
        if custom_words:
            self.spell.word_frequency.load_words(custom_words)

    def check(self, text: str, file_path: str = "") -> List[Typo]:
        """Check text for typos with context awareness."""

        if file_path.endswith('.md'):
            return self._check_markdown(text)
        elif file_path.endswith('.py'):
            return self._check_python_comments(text)
        else:
            return self._check_plain_text(text)

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
                    # Simple line number estimate (not perfect)
                    line_num = node.lineno if hasattr(node, 'lineno') else 1
                    doc_typos = self._check_plain_text(docstring)
                    for typo in doc_typos:
                        typo.line = line_num
                    typos.extend(doc_typos)
        except SyntaxError:
            pass  # Skip files with syntax errors

        return typos

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

    def _check_plain_text(self, text: str) -> List[Typo]:
        """Check plain text (for docstrings)."""
        typos = []
        lines = text.split('\n')

        for line_num, line in enumerate(lines, 1):
            line_typos = self._check_line(line, line_num)
            typos.extend(line_typos)

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

---

### 4. Pattern Layer

```python
import re

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

---

### 5. Repository Scanner

```python
from git import Repo
from pathlib import Path

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
        """Scan repository and return list of files to analyze + current commit SHA.

        Returns:
            (files_to_analyze, current_sha) - DB update deferred to caller
            current_sha will be None if repo has no commits yet
        """
        git_repo = Repo(repo_path)

        # Handle empty repos (no commits yet)
        try:
            current_sha = git_repo.head.commit.hexsha
        except ValueError:
            # Repo has no commits yet - return None instead of "empty" string
            # This prevents BadName errors when trying to do git operations later
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

        # Return files and SHA - caller updates DB after successful scan
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
            '.tox',
            'build',
            'dist',
        ]

        return any(pattern in str(file_path) for pattern in ignore_patterns)
```

---

### 6. Issue Manager

```python
import hashlib

class IssueManager:
    """Manages issue creation with deduplication."""

    def __init__(self, adapter: GiteaAdapter, db_session):
        self.adapter = adapter
        self.db = db_session

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

    def _generate_hash(self, finding: Finding) -> str:
        """Generate unique hash for finding."""
        content = f"{finding.file_path}:{finding.line}:{finding.type}:{finding.message}"
        return hashlib.md5(content.encode()).hexdigest()

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

---

## CLI Design

```python
import click
import asyncio
from pathlib import Path

@click.group()
def cli():
    """drep - Documentation & Review Enhancement Platform"""
    pass

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

@cli.command()
@click.argument('repository')
@click.option('--config', default='config.yaml', help='Config file path')
def scan(repository, config):
    """Scan a repository: drep scan owner/repo"""

    owner, repo_name = repository.split('/')

    click.echo(f"Scanning {owner}/{repo_name}...")

    # Run async scan
    asyncio.run(_run_scan(owner, repo_name, config))

    click.echo("✓ Scan complete")

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

        # Create temporary askpass script for git authentication
        # Script reads token from environment variable (never written to disk)
        import os
        import tempfile

        # Create temp directory for this run only
        temp_dir = tempfile.mkdtemp(prefix='drep_')
        askpass_script = Path(temp_dir) / 'askpass.sh'

        # Script that reads token from DREP_GIT_TOKEN env var
        # This way the token is never written to disk
        askpass_script_content = '''#!/bin/sh
# Git calls this script twice during HTTP auth
# Token is passed via DREP_GIT_TOKEN environment variable

if echo "$1" | grep -qi "username"; then
    # Return "token" as username (Gitea requires non-empty username)
    # Authentication format is: username:token (e.g., token:abc123)
    echo "token"
elif echo "$1" | grep -qi "password"; then
    # Return the token from environment variable
    echo "$DREP_GIT_TOKEN"
else
    # Fallback
    echo "$DREP_GIT_TOKEN"
fi
'''
        askpass_script.write_text(askpass_script_content)
        askpass_script.chmod(0o700)

        # Set up git environment for authentication
        # Token passed via env var (not written to file)
        git_env = {
            **os.environ,  # Keep existing environment (PATH, etc.)
            'GIT_ASKPASS': str(askpass_script),
            'GIT_TERMINAL_PROMPT': '0',
            'DREP_GIT_TOKEN': config.gitea.token,  # Token via env var
        }

        try:
            if not Path(repo_path).exists():
                # Create parent directory first
                Path(repo_path).parent.mkdir(parents=True, exist_ok=True)

                # Get default branch before cloning
                default_branch = await adapter.get_default_branch(owner, repo)

                # Clone with clean URL (no credentials)
                base_url = config.gitea.url.rstrip('/')
                clean_git_url = f"{base_url}/{owner}/{repo}.git"

                # Clone with environment variables
                # Must pass env directly to clone_from
                git_repo = Repo.clone_from(
                    clean_git_url,
                    repo_path,
                    branch=default_branch,
                    env=git_env
                )
            else:
                # Pull latest using environment authentication
                git_repo = Repo(repo_path)
                with git_repo.git.custom_environment(**git_env):
                    git_repo.remotes.origin.pull()

        finally:
            # Always clean up temporary askpass script
            import shutil
            shutil.rmtree(temp_dir, ignore_errors=True)

        # Scan repository (returns files + SHA, doesn't update DB yet)
        files_to_analyze, current_sha = await scanner.scan_repository(repo_path, owner, repo)

        if current_sha is None:
            click.echo("Repository has no commits yet. Skipping.")
            # Don't record scan for empty repos - let next scan check again
            # Once commits exist, they'll be scanned normally
            return

        click.echo(f"Analyzing {len(files_to_analyze)} files...")

        # Analyze files (even if empty list, we still record the scan)
        all_findings = []
        if files_to_analyze:
            for file_path in files_to_analyze:
                full_path = Path(repo_path) / file_path
                content = full_path.read_text()

                findings_obj = await analyzer.analyze_file(str(file_path), content)
                all_findings.extend(findings_obj.to_findings())

            click.echo(f"Found {len(all_findings)} issues")

            # Create issues
            if all_findings:
                await issue_manager.create_issues_for_findings(owner, repo, all_findings)
        else:
            click.echo("No Python/Markdown files to analyze.")

        # Always record scan in DB after successful completion (even if no files)
        # This prevents re-scanning commits that only touched non-doc files
        scanner.record_scan(owner, repo, current_sha)

    finally:
        # Always close HTTP client
        await adapter.close()

if __name__ == '__main__':
    cli()
```

---

## MVP Implementation Phases

### Phase 1 (Week 1): Core Foundation
- [ ] Data models (`drep/models/`)
- [ ] Gitea adapter (`drep/adapters/gitea.py`)
- [ ] Configuration loading
- [ ] Database setup

### Phase 2 (Week 2): Analysis & CLI
- [ ] Spellcheck layer (`drep/documentation/spellcheck.py`)
- [ ] Pattern layer (`drep/documentation/patterns.py`)
- [ ] Documentation analyzer orchestrator
- [ ] Repository scanner with incremental support
- [ ] Issue manager with deduplication
- [ ] CLI commands (`drep init`, `drep scan`)

### Testing
- [ ] Unit tests for spellcheck (markdown, Python comments)
- [ ] Unit tests for pattern matching
- [ ] Integration test: full scan workflow
- [ ] Test incremental scanning
- [ ] Test deduplication

---

## Configuration Example

```yaml
gitea:
  url: http://192.168.1.14:3000
  token: your-gitea-token-here
  repositories:
    - steve/drep
    - steve/my-app

documentation:
  enabled: true
  custom_dictionary:
    - asyncio
    - fastapi
    - gitea
    - pydantic
    - sqlalchemy

database_url: sqlite:///./drep.db
```

---

## Usage Example

```bash
# Initialize configuration
drep init

# Edit config.yaml with your token
vim config.yaml

# Scan a repository
drep scan steve/drep

# Output:
# Scanning steve/drep...
# Analyzing 25 files...
# Found 12 issues
# ✓ Scan complete
# Created 12 issues on Gitea
```

---

## Success Criteria

MVP is complete when:

1. ✅ `drep scan steve/drep` runs without errors
2. ✅ Finds typos in Python comments and docstrings
3. ✅ Finds typos in markdown files
4. ✅ Skips code blocks in markdown
5. ✅ Skips code identifiers (camelCase, snake_case)
6. ✅ Creates issues on Gitea with findings
7. ✅ Doesn't create duplicate issues
8. ✅ Incremental scanning works (only scans changed files)
9. ✅ Completes in < 30 seconds for small repos

---

## Future Phases

### Phase 2: Intelligence (Weeks 3-4)
- Add Layer 3 (LLM integration via open-agent-sdk)
- Missing comment detection and generation
- Bad comment identification
- Draft PR creation with fixes
- Gitea webhooks for automatic scanning
- FastAPI server

### Phase 3: Language Expansion (Weeks 5-6)
- JavaScript/TypeScript support
- JSDoc parsing
- Improved spellcheck accuracy

### Phase 4: Multi-Platform (Weeks 7+)
- GitHub adapter
- GitLab adapter
- Cross-platform testing

### Phase 5: Advanced Features
- Vector database integration
- Go/Rust support
- Custom rule definitions
- Metrics dashboard

---

**End of Technical Design**
