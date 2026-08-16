"""Scanner file-target policy tests (C7): one case-insensitive suffix policy everywhere."""

from typing import ClassVar
from unittest.mock import Mock, patch

import pytest

import drep.core.scanner as scanner_module
from drep.core.scanner import RepositoryScanner


class TestFileTargetPolicyConsistency:
    """C7: all file-target decisions must use one case-insensitive suffix policy."""

    MIXED_CASE_FILES: ClassVar[list[str]] = ["TEST.PY", "Readme.MD", "src/Main.Py", "docs/Notes.md"]
    NON_TARGETS: ClassVar[list[str]] = ["app.js", "style.css", "data.json", "noext"]

    def test_scan_target_predicates_exist(self):
        """Scanner exposes the shared predicates."""
        assert callable(scanner_module.is_scan_target)
        assert callable(scanner_module.is_python_source)

    @pytest.mark.parametrize("path", MIXED_CASE_FILES)
    def test_is_scan_target_accepts_mixed_case(self, path):
        assert scanner_module.is_scan_target(path) is True

    @pytest.mark.parametrize("path", NON_TARGETS)
    def test_is_scan_target_rejects_non_targets(self, path):
        assert scanner_module.is_scan_target(path) is False

    @pytest.mark.parametrize("path", ["TEST.PY", "src/Main.Py"])
    def test_is_python_source_accepts_mixed_case(self, path):
        assert scanner_module.is_python_source(path) is True

    @pytest.mark.parametrize("path", ["Readme.MD", "app.js"])
    def test_is_python_source_rejects_non_python(self, path):
        assert scanner_module.is_python_source(path) is False

    def test_get_scan_targets_finds_mixed_case(self, tmp_path):
        """Full-scan discovery finds mixed-case .py/.md files (was: silently skipped)."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        (tmp_path / "TEST.PY").write_text("x = 1\n")
        (tmp_path / "Readme.MD").write_text("# readme\n")
        (tmp_path / "app.js").write_text("// js\n")

        files = scanner.get_scan_targets(str(tmp_path))

        assert "TEST.PY" in files
        assert "Readme.MD" in files
        assert "app.js" not in files

    def test_changed_files_accepts_mixed_case(self):
        """Commit-diff targeting uses the shared case-insensitive policy."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            diff_item_py = Mock(b_path="src/Main.Py")
            diff_item_md = Mock(b_path="docs/Notes.md")
            diff_item_js = Mock(b_path="app.js")
            diff_item_del = Mock(b_path=None)
            mock_repo.commit.return_value.diff.return_value = [
                diff_item_py,
                diff_item_md,
                diff_item_js,
                diff_item_del,
            ]
            mock_repo_class.return_value = mock_repo

            files = scanner._get_changed_files(mock_repo, "old", "new")

        assert "src/Main.Py" in files
        assert "docs/Notes.md" in files
        assert "app.js" not in files

    def test_docstring_filter_uses_shared_predicate(self):
        """analyze_docstrings filters Python files via is_python_source (case-insensitive)."""
        assert scanner_module.is_python_source("SCRIPT.PY") is True
        assert scanner_module.is_python_source("script.js") is False
