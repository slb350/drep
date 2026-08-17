"""Repository scanner for file-by-file analysis."""

import asyncio
import logging
from collections.abc import Awaitable, Callable
from datetime import datetime, timezone
from pathlib import Path

from git import Repo
from git.exc import GitCommandError, InvalidGitRepositoryError

from drep.adapters.base import BaseAdapter
from drep.code_quality.analyzer import CodeQualityAnalyzer
from drep.core.file_targets import (
    is_ignored_dir,
    is_python_source,
    is_scan_target,
    walk_targets,
)
from drep.core.performance import ProgressTracker
from drep.db.models import RepositoryScan
from drep.docstring.generator import DocstringGenerator
from drep.llm.cache import IntelligentCache
from drep.llm.client import LLMClient
from drep.models.config import Config
from drep.models.findings import AnalysisResult, Finding
from drep.pr_review.analyzer import PRReviewAnalyzer

logger = logging.getLogger(__name__)


class RepositoryScanner:
    """Scans repositories with incremental diff support and optional LLM-powered analysis."""

    # Populated only when LLM analysis is enabled; otherwise None.
    llm_client: LLMClient | None
    code_analyzer: CodeQualityAnalyzer | None
    docstring_generator: DocstringGenerator | None
    pr_analyzer: PRReviewAnalyzer | None

    def __init__(
        self,
        db_session,
        config: Config | None = None,
        adapter: BaseAdapter | None = None,
    ):
        """Initialize scanner with database session and optional config.

        Args:
            db_session: SQLAlchemy database session for querying/storing scan metadata
            config: Optional Config object for LLM-powered analysis
            adapter: Optional platform adapter (Gitea/GitHub/GitLab) for PR review
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

            # Create LLM client with provider detection
            provider = getattr(config.llm, "provider", "openai-compatible")
            bedrock_region = None
            bedrock_model = None

            # Extract Bedrock config if provider is bedrock
            if provider == "bedrock" and config.llm.bedrock:
                bedrock_region = config.llm.bedrock.region
                bedrock_model = config.llm.bedrock.model

            self.llm_client = LLMClient(
                endpoint=str(config.llm.endpoint),
                # For openai-compatible, config validation guarantees a model; for
                # bedrock the model is taken from bedrock_model inside LLMClient.
                model=config.llm.model or "",
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
                provider=provider,
                bedrock_region=bedrock_region,
                bedrock_model=bedrock_model,
            )

            # Create code quality analyzer
            self.code_analyzer = CodeQualityAnalyzer(self.llm_client)

            # Create docstring generator
            self.docstring_generator = DocstringGenerator(self.llm_client)

            # Create PR review analyzer if gitea adapter provided
            if adapter:
                self.pr_analyzer = PRReviewAnalyzer(self.llm_client, adapter)
            else:
                self.pr_analyzer = None
        else:
            self.llm_client = None
            self.code_analyzer = None
            self.docstring_generator = None
            self.pr_analyzer = None

    async def scan_repository(
        self, repo_path: str, owner: str, repo_name: str
    ) -> tuple[list[str], str | None]:
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
            files = self.get_scan_targets(repo_path)

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
            existing.scanned_at = datetime.now(timezone.utc)
        else:
            # Create new record
            new_scan = RepositoryScan(
                owner=owner,
                repo=repo_name,
                commit_sha=commit_sha,
                scanned_at=datetime.now(timezone.utc),
            )
            self.db.add(new_scan)

        self.db.commit()

    def get_scan_targets(self, repo_path: str) -> list[str]:
        """Get all Python and Markdown files in repository.

        Args:
            repo_path: Path to repository root

        Returns:
            List of relative file paths (e.g., ["src/main.py", "README.md"])
        """
        root = Path(repo_path)
        return [str(path.relative_to(root)) for path in walk_targets(root, is_scan_target)]

    def _should_ignore(self, file_path: Path) -> bool:
        """Check if file should be ignored.

        Checks if any path component matches ignore patterns.
        Uses exact directory name matching to avoid false positives.

        Args:
            file_path: Path object to check

        Returns:
            True if file should be ignored, False otherwise
        """
        # Check if any path component is an ignored directory
        return any(is_ignored_dir(part) for part in file_path.parts)

    def _get_changed_files(self, repo: Repo, old_sha: str, new_sha: str) -> list[str]:
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
            if path and is_scan_target(path):
                changed_files.append(path)

        # Deduplicate
        return list(set(changed_files))

    def get_staged_files(self, repo_path: str) -> list[str]:
        """Get staged files from git index (pre-commit workflow).

        Returns only Python (.py) and Markdown (.md) files that are currently
        staged in the git index. Excludes deleted files.

        Args:
            repo_path: Path to git repository root

        Returns:
            List of relative file paths for staged .py and .md files
            (relative to repository root). Returns empty list if no
            matching files staged.

        Raises:
            ValueError: If repo_path is not a valid git repository
            RuntimeError: If git operations fail (corrupted index, etc.)

        Note:
            This method is designed for pre-commit hooks where you only want
            to analyze files that are about to be committed.

            On initial commit (no HEAD exists yet), automatically falls back
            to checking staged files against empty tree.
        """
        # Validate it's a git repository
        try:
            git_repo = Repo(repo_path)
        except InvalidGitRepositoryError as exc:
            logger.error(f"Not a git repository: {repo_path}")
            raise ValueError(
                f"Not a git repository: {repo_path}\n"
                f"drep check --staged requires a git repository.\n"
                f"Try running 'git init' first or use 'drep check' without --staged."
            ) from exc

        staged_files = []

        # Get diff between HEAD and index (staged changes)
        # Note: This will fail on initial commit (no HEAD exists yet).
        # We handle this by falling back to diff against None (empty tree).
        try:
            diff_items = git_repo.index.diff("HEAD")
        except GitCommandError as e:
            if "HEAD" in str(e):
                logger.warning("Repository has no commits yet, checking staged files")
                # Fallback for initial commit - compare against empty tree
                diff_items = git_repo.index.diff(None)
            else:
                logger.error(f"Git operation failed: {e}")
                raise RuntimeError(f"Git operation failed: {e}") from e

        for diff_item in diff_items:
            # Use b_path (current file name) not a_path (old name for renames)
            # b_path is None for deleted files, so we skip those
            path = diff_item.b_path
            if path and is_scan_target(path):
                staged_files.append(path)

        return staged_files

    async def analyze_code_quality(
        self,
        repo_path: str,
        files: list[str],
        repo_id: str,
        commit_sha: str,
        progress_callback: Callable[["ProgressTracker"], None] | None = None,
    ) -> AnalysisResult:
        """Analyze Python files for code quality issues using LLM.

        Args:
            repo_path: Path to repository root
            files: List of file paths to analyze
            repo_id: Repository identifier (e.g., "owner/repo")
            commit_sha: Current commit SHA for cache invalidation
            progress_callback: Optional callback for progress updates

        Returns:
            AnalysisResult with the findings and the files that could not be
            analyzed - callers must not read an empty findings list as "clean"

        Note:
            - Only analyzes if code_analyzer is initialized
            - Only analyzes Python (.py) files
            - Skips files that cannot be read
        """
        if not self.code_analyzer:
            logger.debug("Code analyzer not initialized, skipping code quality analysis")
            return AnalysisResult()

        repo_path_obj = Path(repo_path)

        # Filter to only Python files
        python_files = [f for f in files if self.code_analyzer.is_supported_file(f)]

        if not python_files:
            logger.debug("No Python files to analyze for code quality")
            return AnalysisResult()

        logger.info(f"Analyzing {len(python_files)} Python files for code quality")

        return await self._analyze_files_with(
            analyze=self.code_analyzer.analyze_file,
            repo_path_obj=repo_path_obj,
            files=python_files,
            repo_id=repo_id,
            commit_sha=commit_sha,
            progress_callback=progress_callback,
            what="code quality issues",
        )

    async def analyze_docstrings(
        self,
        repo_path: str,
        files: list[str],
        repo_id: str,
        commit_sha: str,
        progress_callback: Callable[["ProgressTracker"], None] | None = None,
    ) -> AnalysisResult:
        """Analyze Python files for missing/poor docstrings using LLM.

        Args:
            repo_path: Path to repository root
            files: List of file paths to analyze
            repo_id: Repository identifier (e.g., "owner/repo")
            commit_sha: Current commit SHA for cache invalidation
            progress_callback: Optional callback for progress updates

        Returns:
            AnalysisResult with the findings and the files that could not be
            analyzed - callers must not read an empty findings list as "clean"

        Note:
            - Only analyzes if docstring_generator is initialized
            - Only analyzes Python (.py) files
            - Skips files that cannot be read
        """
        if not self.docstring_generator:
            logger.debug("Docstring generator not initialized, skipping docstring analysis")
            return AnalysisResult()

        repo_path_obj = Path(repo_path)

        # Filter to only Python files
        python_files = [f for f in files if is_python_source(f)]

        if not python_files:
            logger.debug("No Python files to analyze for docstrings")
            return AnalysisResult()

        logger.info(f"Analyzing {len(python_files)} Python files for docstrings")

        return await self._analyze_files_with(
            analyze=self.docstring_generator.analyze_file,
            repo_path_obj=repo_path_obj,
            files=python_files,
            repo_id=repo_id,
            commit_sha=commit_sha,
            progress_callback=progress_callback,
            what="docstring issues",
        )

    async def _analyze_files_with(
        self,
        analyze: Callable[..., Awaitable[list[Finding]]],
        repo_path_obj: Path,
        files: list[str],
        repo_id: str,
        commit_sha: str,
        progress_callback: Callable[["ProgressTracker"], None] | None,
        what: str,
    ) -> AnalysisResult:
        """Read each file and run one LLM analyzer over it, concurrently.

        Shared by analyze_code_quality and analyze_docstrings, which differed
        only in the analyzer they called and their log wording.

        The per-file analyses are gathered rather than awaited in sequence: they
        are independent, and the RateLimiter already caps in-flight requests
        (max_concurrent_global / max_concurrent_per_repo). Awaiting one at a
        time made the scan take the *sum* of every LLM round trip while the
        configured concurrency went unused.

        Args:
            analyze: Analyzer coroutine taking file_path/content/repo_id/commit_sha
            repo_path_obj: Repository root
            files: Repo-relative file paths to analyze
            repo_id: Repository identifier, for per-repo rate limiting
            commit_sha: Commit SHA, for cache invalidation
            progress_callback: Optional progress callback
            what: Noun phrase for the summary log line

        Returns:
            AnalysisResult with the combined findings and every file that could
            not be read or analyzed
        """
        tracker = ProgressTracker(total=len(files))
        failed_files: list[str] = []

        def report() -> None:
            if progress_callback:
                progress_callback(tracker)

        def fail(file_path: str) -> None:
            """Record a file as unanalyzed. Returned to the caller, not just counted."""
            failed_files.append(file_path)
            tracker.update(failed=1)

        # Phase 1: read. Local I/O, and it establishes which files are analyzable
        # before any LLM request is issued.
        readable: list[tuple[str, str]] = []
        for file_path in files:
            full_path = repo_path_obj / file_path
            try:
                readable.append((file_path, full_path.read_text(encoding="utf-8")))
                continue
            except FileNotFoundError:
                logger.warning(f"Skipping {file_path}: file not found")
                tracker.update(skipped=1)
            except UnicodeDecodeError:
                logger.warning(f"Skipping {file_path}: Not valid UTF-8")
                tracker.update(skipped=1)
            except PermissionError:
                logger.error(f"Permission denied: {file_path}")
                fail(file_path)
            except OSError as e:
                logger.error(f"Failed to read {file_path}: {e}")
                fail(file_path)
            report()

        # Phase 2: analyze concurrently
        async def analyze_one(file_path: str, content: str) -> list[Finding]:
            try:
                result = await analyze(
                    file_path=file_path,
                    content=content,
                    repo_id=repo_id,
                    commit_sha=commit_sha,
                )
                tracker.update(completed=1)
                return result
            except Exception as e:
                logger.error(f"Failed to analyze {file_path}: {e}")
                fail(file_path)
                return []
            finally:
                report()

        results = await asyncio.gather(*(analyze_one(path, text) for path, text in readable))

        findings = [finding for result in results for finding in result]
        logger.info(f"Found {len(findings)} {what} across {len(files)} files")
        return AnalysisResult(findings=findings, failed_files=failed_files)

    async def close(self):
        """Close LLM client and cleanup resources."""
        if self.llm_client:
            await self.llm_client.close()
