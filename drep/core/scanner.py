"""Repository scanner for file-by-file analysis."""

import logging
from datetime import UTC, datetime
from pathlib import Path
from typing import Callable, List, Optional, Tuple

from git import Repo

from drep.adapters.gitea import GiteaAdapter
from drep.code_quality.analyzer import CodeQualityAnalyzer
from drep.core.performance import ProgressTracker
from drep.db.models import RepositoryScan
from drep.docstring.generator import DocstringGenerator
from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient  # noqa: F401
from drep.models.config import Config
from drep.models.findings import Finding
from drep.pr_review.analyzer import PRReviewAnalyzer

logger = logging.getLogger(__name__)


class RepositoryScanner:
    """Scans repositories with incremental diff support and optional LLM-powered analysis."""

    def __init__(
        self,
        db_session,
        config: Optional[Config] = None,
        gitea_adapter: Optional[GiteaAdapter] = None,
    ):
        """Initialize scanner with database session and optional config.

        Args:
            db_session: SQLAlchemy database session for querying/storing scan metadata
            config: Optional Config object for LLM-powered analysis
            gitea_adapter: Optional GiteaAdapter for PR review functionality
        """
        self.db = db_session
        self.config = config

        # Initialize LLM client and code analyzer if enabled
        if config and config.llm and config.llm.enabled:
            logger.info("Initializing LLM client for code quality analysis")

            # Create cache if enabled
            cache = None
            if config.llm.cache.enabled:
                cache = IntelligentCache(
                    cache_dir=config.llm.cache.directory,
                    ttl_days=config.llm.cache.ttl_days,
                    max_size_bytes=int(config.llm.cache.max_size_gb * 1024**3),
                )

            # Create LLM client
            self.llm_client = LLMClient(
                endpoint=str(config.llm.endpoint),
                model=config.llm.model,
                api_key=config.llm.api_key,
                temperature=config.llm.temperature,
                max_tokens=config.llm.max_tokens,
                timeout=config.llm.timeout,
                max_retries=config.llm.max_retries,
                retry_delay=config.llm.retry_delay,
                exponential_backoff=config.llm.exponential_backoff,
                max_concurrent_global=config.llm.max_concurrent_global,
                max_concurrent_per_repo=config.llm.max_concurrent_per_repo,
                requests_per_minute=config.llm.requests_per_minute,
                max_tokens_per_minute=config.llm.max_tokens_per_minute,
                cache=cache,
            )

            # Create code quality analyzer
            self.code_analyzer = CodeQualityAnalyzer(self.llm_client)

            # Create docstring generator
            self.docstring_generator = DocstringGenerator(self.llm_client)

            # Create PR review analyzer if gitea adapter provided
            if gitea_adapter:
                self.pr_analyzer = PRReviewAnalyzer(self.llm_client, gitea_adapter)
            else:
                self.pr_analyzer = None
        else:
            self.llm_client = None
            self.code_analyzer = None
            self.docstring_generator = None
            self.pr_analyzer = None

    async def scan_repository(
        self, repo_path: str, owner: str, repo_name: str
    ) -> Tuple[List[str], Optional[str]]:
        """Scan repository and return list of files + commit SHA.

        Args:
            repo_path: Path to local git repository
            owner: Repository owner (e.g., "steve")
            repo_name: Repository name (e.g., "drep")

        Returns:
            Tuple of (list of file paths to analyze, current commit SHA)
            Returns ([], None) for empty repositories with no commits
        """
        git_repo = Repo(repo_path)

        # Handle empty repos (no commits yet)
        try:
            current_sha = git_repo.head.commit.hexsha
        except (ValueError, AttributeError):
            # Repo has no commits yet
            return ([], None)

        # Get last scan
        last_scan = (
            self.db.query(RepositoryScan)
            .filter_by(owner=owner, repo=repo_name)
            .order_by(RepositoryScan.scanned_at.desc())
            .first()
        )

        if last_scan:
            # Incremental scan - only changed files
            files = self._get_changed_files(git_repo, last_scan.commit_sha, current_sha)
        else:
            # Full scan - all Python/Markdown files
            files = self._get_all_python_files(repo_path)

        return (files, current_sha)

    def record_scan(self, owner: str, repo_name: str, commit_sha: str):
        """Record successful scan in database.

        Updates existing record if one exists for this owner/repo,
        otherwise creates a new record.

        Args:
            owner: Repository owner
            repo_name: Repository name
            commit_sha: Git commit SHA that was scanned
        """
        # Check if record already exists
        existing = self.db.query(RepositoryScan).filter_by(owner=owner, repo=repo_name).first()

        if existing:
            # Update existing record
            existing.commit_sha = commit_sha
            existing.scanned_at = datetime.now(UTC)
        else:
            # Create new record
            new_scan = RepositoryScan(
                owner=owner,
                repo=repo_name,
                commit_sha=commit_sha,
                scanned_at=datetime.now(UTC),
            )
            self.db.add(new_scan)

        self.db.commit()

    def _get_all_python_files(self, repo_path: str) -> List[str]:
        """Get all Python and Markdown files in repository.

        Args:
            repo_path: Path to repository root

        Returns:
            List of relative file paths (e.g., ["src/main.py", "README.md"])
        """
        files = []
        repo_path = Path(repo_path)

        for pattern in ["**/*.py", "**/*.md"]:
            files.extend(
                [
                    str(f.relative_to(repo_path))
                    for f in repo_path.glob(pattern)
                    if not self._should_ignore(f)
                ]
            )

        return files

    def _should_ignore(self, file_path: Path) -> bool:
        """Check if file should be ignored.

        Checks if any path component matches ignore patterns.
        Uses exact directory name matching to avoid false positives.

        Args:
            file_path: Path object to check

        Returns:
            True if file should be ignored, False otherwise
        """
        ignore_dirs = {
            "__pycache__",
            ".git",
            "venv",
            "env",
            ".venv",
            ".tox",
            "build",
            "dist",
            ".eggs",
        }

        # Check if any path component is an ignored directory
        for part in file_path.parts:
            if part in ignore_dirs:
                return True
            # Check for .egg-info directories (e.g., drep.egg-info)
            if part.endswith(".egg-info"):
                return True

        return False

    def _get_changed_files(self, repo: Repo, old_sha: str, new_sha: str) -> List[str]:
        """Get files changed between two commits.

        Only returns files that exist in the new commit (excludes deleted files
        and old names of renamed files).

        Args:
            repo: GitPython Repo object
            old_sha: Old commit SHA
            new_sha: New commit SHA

        Returns:
            List of changed file paths that are .py or .md files
        """
        diff = repo.commit(old_sha).diff(new_sha)

        changed_files = []
        for diff_item in diff:
            # Only use b_path (the file path in the new commit)
            # This excludes deleted files (b_path is None) and old names of renames
            path = diff_item.b_path
            if path and (path.endswith(".py") or path.endswith(".md")):
                changed_files.append(path)

        # Deduplicate
        return list(set(changed_files))

    async def analyze_code_quality(
        self,
        repo_path: str,
        files: List[str],
        repo_id: str,
        commit_sha: str,
        progress_callback: Optional[Callable[["ProgressTracker"], None]] = None,
    ) -> List[Finding]:
        """Analyze Python files for code quality issues using LLM.

        Args:
            repo_path: Path to repository root
            files: List of file paths to analyze
            repo_id: Repository identifier (e.g., "owner/repo")
            commit_sha: Current commit SHA for cache invalidation
            progress_callback: Optional callback for progress updates

        Returns:
            List of Finding objects describing code quality issues

        Note:
            - Only analyzes if code_analyzer is initialized
            - Only analyzes Python (.py) files
            - Skips files that cannot be read
        """
        if not self.code_analyzer:
            logger.debug("Code analyzer not initialized, skipping code quality analysis")
            return []

        findings: List[Finding] = []
        repo_path_obj = Path(repo_path)

        # Filter to only Python files
        python_files = [f for f in files if self.code_analyzer.is_supported_file(f)]

        if not python_files:
            logger.debug("No Python files to analyze for code quality")
            return []

        logger.info(f"Analyzing {len(python_files)} Python files for code quality")

        # Initialize progress tracker
        from drep.core.performance import ProgressTracker

        tracker = ProgressTracker(total=len(python_files))

        # Analyze each file
        for file_path in python_files:
            full_path = repo_path_obj / file_path

            # Skip if file doesn't exist
            if not full_path.exists():
                logger.warning(f"Skipping {file_path}: file not found")
                tracker.update(skipped=1)
                if progress_callback:
                    progress_callback(tracker)
                continue

            # Read file content
            try:
                content = full_path.read_text(errors="ignore")
            except Exception as e:
                logger.error(f"Failed to read {file_path}: {e}")
                tracker.update(failed=1)
                if progress_callback:
                    progress_callback(tracker)
                continue

            # Analyze with code analyzer
            try:
                file_findings = await self.code_analyzer.analyze_file(
                    file_path=file_path, content=content, repo_id=repo_id, commit_sha=commit_sha
                )
                findings.extend(file_findings)
                tracker.update(completed=1)
            except Exception as e:
                logger.error(f"Failed to analyze {file_path}: {e}")
                tracker.update(failed=1)

            # Call progress callback
            if progress_callback:
                progress_callback(tracker)

        logger.info(f"Found {len(findings)} code quality issues across {len(python_files)} files")
        return findings

    async def analyze_docstrings(
        self,
        repo_path: str,
        files: List[str],
        repo_id: str,
        commit_sha: str,
        progress_callback: Optional[Callable[["ProgressTracker"], None]] = None,
    ) -> List[Finding]:
        """Analyze Python files for missing/poor docstrings using LLM.

        Args:
            repo_path: Path to repository root
            files: List of file paths to analyze
            repo_id: Repository identifier (e.g., "owner/repo")
            commit_sha: Current commit SHA for cache invalidation
            progress_callback: Optional callback for progress updates

        Returns:
            List of Finding objects for docstring issues

        Note:
            - Only analyzes if docstring_generator is initialized
            - Only analyzes Python (.py) files
            - Skips files that cannot be read
        """
        if not self.docstring_generator:
            logger.debug("Docstring generator not initialized, skipping docstring analysis")
            return []

        findings: List[Finding] = []
        repo_path_obj = Path(repo_path)

        # Filter to only Python files
        python_files = [f for f in files if f.endswith(".py")]

        if not python_files:
            logger.debug("No Python files to analyze for docstrings")
            return []

        logger.info(f"Analyzing {len(python_files)} Python files for docstrings")

        # Initialize progress tracker
        from drep.core.performance import ProgressTracker

        tracker = ProgressTracker(total=len(python_files))

        # Analyze each file
        for file_path in python_files:
            full_path = repo_path_obj / file_path

            # Skip if file doesn't exist
            if not full_path.exists():
                logger.warning(f"Skipping {file_path}: file not found")
                tracker.update(skipped=1)
                if progress_callback:
                    progress_callback(tracker)
                continue

            # Read file content
            try:
                content = full_path.read_text(errors="ignore")
            except Exception as e:
                logger.error(f"Failed to read {file_path}: {e}")
                tracker.update(failed=1)
                if progress_callback:
                    progress_callback(tracker)
                continue

            # Analyze with docstring generator
            try:
                file_findings = await self.docstring_generator.analyze_file(
                    file_path=file_path, content=content, repo_id=repo_id, commit_sha=commit_sha
                )
                findings.extend(file_findings)
                tracker.update(completed=1)
            except Exception as e:
                logger.error(f"Failed to analyze {file_path}: {e}")
                tracker.update(failed=1)

            # Call progress callback
            if progress_callback:
                progress_callback(tracker)

        logger.info(f"Found {len(findings)} docstring issues across {len(python_files)} files")
        return findings

    async def close(self):
        """Close LLM client and cleanup resources."""
        if self.llm_client:
            await self.llm_client.close()
