"""Repository scanner for file-by-file analysis."""

from datetime import UTC, datetime
from pathlib import Path
from typing import List, Optional, Tuple

from git import Repo

from drep.db.models import RepositoryScan


class RepositoryScanner:
    """Scans repositories with incremental diff support."""

    def __init__(self, db_session):
        """Initialize scanner with database session.

        Args:
            db_session: SQLAlchemy database session for querying/storing scan metadata
        """
        self.db = db_session

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
