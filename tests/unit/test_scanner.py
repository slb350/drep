"""Unit tests for RepositoryScanner."""

from pathlib import Path
from unittest.mock import Mock, PropertyMock, patch

import pytest

from drep.core.scanner import RepositoryScanner


class TestRepositoryScannerBasicStructure:
    """Tests for RepositoryScanner initialization and basic structure."""

    def test_scanner_instantiation(self):
        """Test that RepositoryScanner can be instantiated with a database session."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        assert scanner is not None

    def test_scanner_stores_db_session(self):
        """Test that RepositoryScanner stores the database session."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        assert scanner.db is db_session

    @pytest.mark.asyncio
    async def test_scan_repository_method_exists(self):
        """Test that scan_repository method exists and returns tuple."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Method should exist
        assert hasattr(scanner, "scan_repository")
        assert callable(scanner.scan_repository)

    @pytest.mark.asyncio
    async def test_scan_repository_returns_tuple(self):
        """Test that scan_repository returns a tuple of (files, sha)."""
        db_session = Mock()
        # Mock database query to return None (no previous scan)
        query_mock = db_session.query.return_value.filter_by.return_value
        query_mock.order_by.return_value.first.return_value = None
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            mock_repo.head.commit.hexsha = "abc123"
            mock_repo_class.return_value = mock_repo

            # Mock _get_all_python_files to avoid file system access
            scanner._get_all_python_files = Mock(return_value=[])

            # Should return tuple (even if stub implementation)
            result = await scanner.scan_repository("/fake/path", "owner", "repo")
            assert isinstance(result, tuple)
            assert len(result) == 2

    def test_record_scan_method_exists(self):
        """Test that record_scan method exists."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        assert hasattr(scanner, "record_scan")
        assert callable(scanner.record_scan)


class TestGetAllPythonFiles:
    """Tests for _get_all_python_files method."""

    def test_get_all_python_files_method_exists(self):
        """Test that _get_all_python_files method exists."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        assert hasattr(scanner, "_get_all_python_files")
        assert callable(scanner._get_all_python_files)

    def test_get_all_python_files_finds_python_files(self, tmp_path):
        """Test that _get_all_python_files finds .py files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Create test files
        (tmp_path / "test.py").write_text("# test")
        (tmp_path / "main.py").write_text("# main")
        (tmp_path / "README.md").write_text("# readme")

        files = scanner._get_all_python_files(str(tmp_path))

        # Should find .py and .md files
        assert len(files) >= 2
        assert any("test.py" in f for f in files)
        assert any("main.py" in f for f in files)

    def test_get_all_python_files_finds_markdown_files(self, tmp_path):
        """Test that _get_all_python_files finds .md files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Create test files
        (tmp_path / "README.md").write_text("# readme")
        (tmp_path / "CHANGELOG.md").write_text("# changelog")

        files = scanner._get_all_python_files(str(tmp_path))

        # Should find .md files
        assert len(files) >= 2
        assert any("README.md" in f for f in files)
        assert any("CHANGELOG.md" in f for f in files)

    def test_get_all_python_files_ignores_venv(self, tmp_path):
        """Test that _get_all_python_files ignores venv directory."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Create files in venv
        venv_dir = tmp_path / "venv" / "lib"
        venv_dir.mkdir(parents=True)
        (venv_dir / "test.py").write_text("# venv file")

        # Create file in root
        (tmp_path / "main.py").write_text("# main")

        files = scanner._get_all_python_files(str(tmp_path))

        # Should not include venv files
        assert not any("venv" in f for f in files)
        assert any("main.py" in f for f in files)

    def test_get_all_python_files_ignores_pycache(self, tmp_path):
        """Test that _get_all_python_files ignores __pycache__ directory."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Create files in __pycache__
        cache_dir = tmp_path / "__pycache__"
        cache_dir.mkdir()
        (cache_dir / "test.pyc").write_text("# cache")

        # Create file in root
        (tmp_path / "main.py").write_text("# main")

        files = scanner._get_all_python_files(str(tmp_path))

        # Should not include __pycache__ files
        assert not any("__pycache__" in f for f in files)

    def test_should_ignore_method_exists(self):
        """Test that _should_ignore helper method exists."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        assert hasattr(scanner, "_should_ignore")
        assert callable(scanner._should_ignore)

    def test_should_ignore_venv_directories(self):
        """Test that _should_ignore returns True for venv paths."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        venv_path = Path("venv/lib/python3.10/test.py")
        assert scanner._should_ignore(venv_path) is True

    def test_should_ignore_pycache_directories(self):
        """Test that _should_ignore returns True for __pycache__ paths."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        cache_path = Path("src/__pycache__/test.pyc")
        assert scanner._should_ignore(cache_path) is True

    def test_should_ignore_allows_normal_files(self):
        """Test that _should_ignore returns False for normal paths."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        normal_path = Path("src/main.py")
        assert scanner._should_ignore(normal_path) is False


class TestGetChangedFiles:
    """Tests for _get_changed_files method."""

    def test_get_changed_files_method_exists(self):
        """Test that _get_changed_files method exists."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        assert hasattr(scanner, "_get_changed_files")
        assert callable(scanner._get_changed_files)

    def test_get_changed_files_filters_python_files(self):
        """Test that _get_changed_files only returns .py and .md files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Mock git repo
        mock_repo = Mock()
        mock_diff_item1 = Mock()
        mock_diff_item1.a_path = "test.py"
        mock_diff_item1.b_path = "test.py"

        mock_diff_item2 = Mock()
        mock_diff_item2.a_path = "README.md"
        mock_diff_item2.b_path = "README.md"

        mock_diff_item3 = Mock()
        mock_diff_item3.a_path = "test.txt"
        mock_diff_item3.b_path = "test.txt"

        mock_commit = Mock()
        mock_commit.diff.return_value = [mock_diff_item1, mock_diff_item2, mock_diff_item3]
        mock_repo.commit.return_value = mock_commit

        files = scanner._get_changed_files(mock_repo, "sha1", "sha2")

        # Should only include .py and .md files
        assert "test.py" in files
        assert "README.md" in files
        assert "test.txt" not in files

    def test_get_changed_files_handles_renames(self):
        """Test that _get_changed_files handles renamed files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Mock renamed file (a_path != b_path)
        mock_repo = Mock()
        mock_diff_item = Mock()
        mock_diff_item.a_path = "old_name.py"
        mock_diff_item.b_path = "new_name.py"

        mock_commit = Mock()
        mock_commit.diff.return_value = [mock_diff_item]
        mock_repo.commit.return_value = mock_commit

        files = scanner._get_changed_files(mock_repo, "sha1", "sha2")

        # Should include both old and new names
        assert "old_name.py" in files or "new_name.py" in files

    def test_get_changed_files_deduplicates(self):
        """Test that _get_changed_files removes duplicates."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Mock multiple items with same file
        mock_repo = Mock()
        mock_diff_item1 = Mock()
        mock_diff_item1.a_path = "test.py"
        mock_diff_item1.b_path = "test.py"

        mock_diff_item2 = Mock()
        mock_diff_item2.a_path = "test.py"
        mock_diff_item2.b_path = "test.py"

        mock_commit = Mock()
        mock_commit.diff.return_value = [mock_diff_item1, mock_diff_item2]
        mock_repo.commit.return_value = mock_commit

        files = scanner._get_changed_files(mock_repo, "sha1", "sha2")

        # Should deduplicate
        assert files.count("test.py") == 1


class TestScanRepositoryMainLogic:
    """Tests for main scan_repository logic."""

    @pytest.mark.asyncio
    async def test_scan_repository_handles_empty_repo(self):
        """Test that scan_repository handles repositories with no commits."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            # Mock empty repo (no commits) - accessing hexsha raises ValueError
            mock_repo = Mock()
            type(mock_repo.head.commit).hexsha = PropertyMock(
                side_effect=ValueError("Reference 'refs/heads/main' not found")
            )
            mock_repo_class.return_value = mock_repo

            files, sha = await scanner.scan_repository("/fake/path", "owner", "repo")

            # Should return empty list and None
            assert files == []
            assert sha is None

    @pytest.mark.asyncio
    async def test_scan_repository_full_scan_when_no_previous_scan(self, tmp_path):
        """Test that scan_repository does full scan when no previous scan exists."""
        db_session = Mock()
        # Mock database query to return None (no previous scan)
        query_mock = db_session.query.return_value.filter_by.return_value
        query_mock.order_by.return_value.first.return_value = None

        scanner = RepositoryScanner(db_session)

        # Create test files
        (tmp_path / "test.py").write_text("# test")
        (tmp_path / ".git").mkdir()

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            mock_repo.head.commit.hexsha = "abc123"
            mock_repo_class.return_value = mock_repo

            # Mock _get_all_python_files to return files
            scanner._get_all_python_files = Mock(return_value=["test.py"])

            files, sha = await scanner.scan_repository(str(tmp_path), "owner", "repo")

            # Should call _get_all_python_files (full scan)
            scanner._get_all_python_files.assert_called_once()
            assert len(files) > 0
            assert sha == "abc123"

    @pytest.mark.asyncio
    async def test_scan_repository_incremental_scan_when_previous_scan_exists(self):
        """Test that scan_repository does incremental scan when previous scan exists."""
        # Mock database with previous scan
        mock_previous_scan = Mock()
        mock_previous_scan.commit_sha = "old_sha"

        db_session = Mock()
        # Mock database query to return previous scan
        query_mock = db_session.query.return_value.filter_by.return_value
        query_mock.order_by.return_value.first.return_value = mock_previous_scan

        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            mock_repo.head.commit.hexsha = "new_sha"
            mock_repo_class.return_value = mock_repo

            # Mock _get_changed_files
            scanner._get_changed_files = Mock(return_value=["changed.py"])

            files, sha = await scanner.scan_repository("/fake/path", "owner", "repo")

            # Should call _get_changed_files (incremental scan)
            scanner._get_changed_files.assert_called_once_with(mock_repo, "old_sha", "new_sha")
            assert files == ["changed.py"]
            assert sha == "new_sha"

    def test_record_scan_creates_database_entry(self):
        """Test that record_scan creates a RepositoryScan entry when none exists."""
        db_session = Mock()
        # Mock query to return None (no existing record)
        db_session.query.return_value.filter_by.return_value.first.return_value = None
        scanner = RepositoryScanner(db_session)

        scanner.record_scan("owner", "repo", "abc123")

        # Should add entry to database (new record)
        db_session.add.assert_called_once()
        db_session.commit.assert_called_once()

    def test_record_scan_stores_correct_data(self):
        """Test that record_scan stores owner, repo, and commit_sha."""
        db_session = Mock()
        # Mock query to return None (no existing record)
        db_session.query.return_value.filter_by.return_value.first.return_value = None
        scanner = RepositoryScanner(db_session)

        scanner.record_scan("test_owner", "test_repo", "commit_sha_123")

        # Check that add was called with RepositoryScan object
        call_args = db_session.add.call_args
        assert call_args is not None

        # Get the object that was added
        scan_obj = call_args[0][0]
        assert scan_obj.owner == "test_owner"
        assert scan_obj.repo == "test_repo"
        assert scan_obj.commit_sha == "commit_sha_123"


class TestBugFixes:
    """Tests for bug fixes identified in code review."""

    def test_record_scan_handles_duplicate_owner_repo(self):
        """Test that record_scan updates existing record instead of violating unique constraint.

        Bug: record_scan always creates new RepositoryScan, violating uq_owner_repo
        unique constraint on subsequent scans.
        """
        from drep.db import init_database
        from drep.db.models import RepositoryScan as RepoScan

        # Use real database to test unique constraint
        db = init_database("sqlite:///:memory:")
        scanner = RepositoryScanner(db)

        # First scan - should succeed
        scanner.record_scan("owner", "repo", "sha1")

        # Second scan with same owner/repo - should update, not crash
        scanner.record_scan("owner", "repo", "sha2")

        # Should have exactly one record with latest SHA
        scans = db.query(RepoScan).filter_by(owner="owner", repo="repo").all()
        assert len(scans) == 1
        assert scans[0].commit_sha == "sha2"

    def test_get_changed_files_filters_deleted_files(self):
        """Test that _get_changed_files excludes deleted files.

        Bug: _get_changed_files returns deleted/renamed-away files that cause
        FileNotFoundError when trying to read them.
        """
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Mock git repo with deleted file
        mock_repo = Mock()

        # Mock diff item for deleted file (exists in a_path but not b_path)
        mock_deleted = Mock()
        mock_deleted.a_path = "deleted.py"
        mock_deleted.b_path = None
        mock_deleted.deleted_file = True

        # Mock diff item for renamed file (old name in a_path, new in b_path)
        mock_renamed = Mock()
        mock_renamed.a_path = "old_name.py"
        mock_renamed.b_path = "new_name.py"
        mock_renamed.renamed_file = True

        # Mock diff item for modified file (exists in both)
        mock_modified = Mock()
        mock_modified.a_path = "modified.py"
        mock_modified.b_path = "modified.py"

        mock_commit = Mock()
        mock_commit.diff.return_value = [mock_deleted, mock_renamed, mock_modified]
        mock_repo.commit.return_value = mock_commit

        files = scanner._get_changed_files(mock_repo, "sha1", "sha2")

        # Should NOT include deleted.py or old_name.py
        # Should include new_name.py and modified.py
        assert "deleted.py" not in files
        assert "old_name.py" not in files
        assert "new_name.py" in files or "modified.py" in files

    def test_should_ignore_no_false_positives_for_env(self):
        """Test that _should_ignore doesn't match 'env' in environment.py.

        Bug: Substring matching causes false positives - 'env' matches
        'environment.py', 'build' matches 'building.md', etc.
        """
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # These should NOT be ignored (false positives in original code)
        assert scanner._should_ignore(Path("src/environment.py")) is False
        assert scanner._should_ignore(Path("docs/building.md")) is False
        assert scanner._should_ignore(Path("src/config/development.py")) is False

    def test_should_ignore_matches_actual_directories(self):
        """Test that _should_ignore correctly matches directory names."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # These SHOULD be ignored
        assert scanner._should_ignore(Path("venv/lib/test.py")) is True
        assert scanner._should_ignore(Path("env/bin/test.py")) is True
        assert scanner._should_ignore(Path("build/lib/test.py")) is True
        assert scanner._should_ignore(Path("__pycache__/test.pyc")) is True

    def test_should_ignore_handles_egg_info_wildcard(self):
        """Test that _should_ignore matches .egg-info directories.

        Bug: Literal '*.egg-info' string never matches because * isn't
        treated as a wildcard.
        """
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        # Should match actual .egg-info directories
        assert scanner._should_ignore(Path("drep.egg-info/PKG-INFO")) is True
        assert scanner._should_ignore(Path("my_package.egg-info/SOURCES.txt")) is True


class TestDocstringAnalysis:
    """Tests for docstring analysis integration in scanner."""

    @pytest.mark.asyncio
    async def test_analyze_docstrings_without_llm(self):
        """Test that analyze_docstrings returns empty list when LLM not enabled."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)  # No config = no LLM

        findings = await scanner.analyze_docstrings(
            repo_path="/fake/path",
            files=["module.py"],
            repo_id="test/repo",
            commit_sha="abc123",
        )

        assert findings == []

    @pytest.mark.asyncio
    async def test_analyze_docstrings_filters_python_files(self, tmp_path):
        """Test that analyze_docstrings only processes Python files."""
        from unittest.mock import AsyncMock

        from drep.models.config import Config, LLMConfig

        db_session = Mock()

        # Create minimal config with LLM enabled
        config = Mock(spec=Config)
        config.llm = Mock(spec=LLMConfig)
        config.llm.enabled = True
        config.llm.endpoint = "http://localhost:8000"
        config.llm.model = "test-model"
        config.llm.cache = Mock()
        config.llm.cache.enabled = False
        config.llm.api_key = None
        config.llm.temperature = 0.2
        config.llm.max_tokens = 1000
        config.llm.timeout = 60
        config.llm.max_retries = 3
        config.llm.retry_delay = 1
        config.llm.exponential_backoff = True
        config.llm.max_concurrent_global = 5
        config.llm.max_concurrent_per_repo = 3
        config.llm.requests_per_minute = 60
        config.llm.max_tokens_per_minute = 100000

        scanner = RepositoryScanner(db_session, config)

        # Mock the docstring_generator.analyze_file method
        scanner.docstring_generator.analyze_file = AsyncMock(return_value=[])

        # Create test files
        py_file = tmp_path / "test.py"
        py_file.write_text("def func(): pass")
        md_file = tmp_path / "README.md"
        md_file.write_text("# Readme")

        # Analyze with mixed file types
        files = ["test.py", "README.md"]
        await scanner.analyze_docstrings(
            repo_path=str(tmp_path),
            files=files,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should only call analyze_file for Python file
        assert scanner.docstring_generator.analyze_file.call_count == 1
        call_args = scanner.docstring_generator.analyze_file.call_args
        assert call_args.kwargs["file_path"] == "test.py"


class TestGetStagedFiles:
    """Tests for get_staged_files method (pre-commit integration)."""

    def test_get_staged_files_method_exists(self):
        """Test that get_staged_files method exists."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        assert hasattr(scanner, "get_staged_files")
        assert callable(scanner.get_staged_files)

    def test_get_staged_files_returns_only_python_and_markdown(self):
        """Test that get_staged_files returns only .py and .md files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # Mock staged files: Python, Markdown, and other
            mock_diff_item1 = Mock()
            mock_diff_item1.a_path = "test.py"
            mock_diff_item1.b_path = "test.py"

            mock_diff_item2 = Mock()
            mock_diff_item2.a_path = "README.md"
            mock_diff_item2.b_path = "README.md"

            mock_diff_item3 = Mock()
            mock_diff_item3.a_path = "config.json"
            mock_diff_item3.b_path = "config.json"

            # Mock index.diff("HEAD") to return staged changes
            mock_repo.index.diff.return_value = [
                mock_diff_item1,
                mock_diff_item2,
                mock_diff_item3,
            ]
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should only include .py and .md files
            assert "test.py" in files
            assert "README.md" in files
            assert "config.json" not in files

    def test_get_staged_files_handles_new_files(self):
        """Test that get_staged_files includes newly added files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # Mock new file (a_path is None)
            mock_new_file = Mock()
            mock_new_file.a_path = None
            mock_new_file.b_path = "new_file.py"

            mock_repo.index.diff.return_value = [mock_new_file]
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should include new file
            assert "new_file.py" in files

    def test_get_staged_files_handles_deleted_files(self):
        """Test that get_staged_files excludes deleted files."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # Mock deleted file (b_path is None)
            mock_deleted_file = Mock()
            mock_deleted_file.a_path = "deleted.py"
            mock_deleted_file.b_path = None

            # Mock modified file (should be included)
            mock_modified_file = Mock()
            mock_modified_file.a_path = "modified.py"
            mock_modified_file.b_path = "modified.py"

            mock_repo.index.diff.return_value = [
                mock_deleted_file,
                mock_modified_file,
            ]
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should NOT include deleted file
            assert "deleted.py" not in files
            # Should include modified file
            assert "modified.py" in files

    def test_get_staged_files_handles_renamed_files(self):
        """Test that get_staged_files handles renamed files correctly."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # Mock renamed file (a_path != b_path)
            mock_renamed = Mock()
            mock_renamed.a_path = "old_name.py"
            mock_renamed.b_path = "new_name.py"

            mock_repo.index.diff.return_value = [mock_renamed]
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should include new name, not old name
            assert "new_name.py" in files
            assert "old_name.py" not in files

    def test_get_staged_files_returns_empty_when_nothing_staged(self):
        """Test that get_staged_files returns empty list when nothing staged."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            mock_repo.index.diff.return_value = []
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            assert files == []

    def test_get_staged_files_raises_on_non_git_repository(self):
        """Test that get_staged_files raises ValueError for non-git directory."""
        from git.exc import InvalidGitRepositoryError

        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo_class.side_effect = InvalidGitRepositoryError("/not/git")

            with pytest.raises(ValueError) as exc_info:
                scanner.get_staged_files("/not/git")

            assert "Not a git repository" in str(exc_info.value)
            assert "/not/git" in str(exc_info.value)
            assert "git init" in str(exc_info.value)

    def test_get_staged_files_handles_initial_commit(self):
        """Test that get_staged_files handles initial commit (no HEAD)."""
        from git.exc import GitCommandError

        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # First call to diff("HEAD") raises GitCommandError for HEAD
            # Second call to diff(None) returns staged files
            def diff_side_effect(ref):
                if ref == "HEAD":
                    raise GitCommandError("git diff", 128, stderr="unknown revision 'HEAD'")
                else:
                    # Return some staged files on fallback
                    mock_diff_item = Mock()
                    mock_diff_item.b_path = "new_file.py"
                    return [mock_diff_item]

            mock_repo.index.diff.side_effect = diff_side_effect
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should fall back to diff(None) for initial commit
            assert files == ["new_file.py"]
            assert mock_repo.index.diff.call_count == 2
            mock_repo.index.diff.assert_any_call("HEAD")
            mock_repo.index.diff.assert_any_call(None)

    def test_get_staged_files_raises_on_git_command_error(self):
        """Test that get_staged_files raises RuntimeError for other git errors."""
        from git.exc import GitCommandError

        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            # Git error that's NOT about HEAD
            mock_repo.index.diff.side_effect = GitCommandError(
                "git diff", 128, stderr="corrupted index"
            )
            mock_repo_class.return_value = mock_repo

            with pytest.raises(RuntimeError) as exc_info:
                scanner.get_staged_files("/fake/path")

            assert "Git operation failed" in str(exc_info.value)

    def test_get_staged_files_handles_uppercase_extensions(self):
        """Test that get_staged_files handles uppercase file extensions (.PY, .MD)."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)

        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()

            # Create mock diff items with uppercase extensions
            mock_diff_items = []
            for filename in ["TEST.PY", "README.MD", "script.Py", "doc.Md", "other.TXT"]:
                mock_diff_item = Mock()
                mock_diff_item.b_path = filename
                mock_diff_items.append(mock_diff_item)

            mock_repo.index.diff.return_value = mock_diff_items
            mock_repo_class.return_value = mock_repo

            files = scanner.get_staged_files("/fake/path")

            # Should match case-insensitively
            assert "TEST.PY" in files
            assert "README.MD" in files
            assert "script.Py" in files
            assert "doc.Md" in files
            assert "other.TXT" not in files  # Wrong extension
            assert len(files) == 4
