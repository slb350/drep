"""Scanner file-target policy tests (C7): one case-insensitive suffix policy everywhere."""

from typing import ClassVar
from unittest.mock import Mock, patch

import pytest

import drep.core.scanner as scanner_module
from drep.core.scanner import RepositoryScanner


class TestFileTargetPolicyConsistency:
    """C7: all file-target decisions must use one case-insensitive suffix policy."""

    MIXED_CASE_FILES: ClassVar[list[str]] = ["TEST.PY", "Readme.MD", "src/Main.Py", "docs/Notes.md"]
    # .js became a target when the language registry landed; these are types no
    # registered language claims.
    NON_TARGETS: ClassVar[list[str]] = ["style.css", "data.json", "noext", "notes.txt"]

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

    @pytest.mark.parametrize("path", ["Readme.MD", "style.css"])
    def test_is_python_source_rejects_non_python(self, path):
        assert scanner_module.is_python_source(path) is False

    def test_get_scan_targets_finds_mixed_case(self, tmp_path):
        """Full-scan discovery finds mixed-case .py/.md files (was: silently skipped)."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        (tmp_path / "TEST.PY").write_text("x = 1\n")
        (tmp_path / "Readme.MD").write_text("# readme\n")
        (tmp_path / "style.css").write_text("body {}\n")

        files = scanner.get_scan_targets(str(tmp_path))

        assert "TEST.PY" in files
        assert "Readme.MD" in files
        assert "style.css" not in files

    def test_changed_files_accepts_mixed_case(self):
        """Commit-diff targeting uses the shared case-insensitive policy."""
        db_session = Mock()
        scanner = RepositoryScanner(db_session)
        with patch("drep.core.scanner.Repo") as mock_repo_class:
            mock_repo = Mock()
            diff_item_py = Mock(b_path="src/Main.Py")
            diff_item_md = Mock(b_path="docs/Notes.md")
            diff_item_css = Mock(b_path="style.css")
            diff_item_del = Mock(b_path=None)
            mock_repo.commit.return_value.diff.return_value = [
                diff_item_py,
                diff_item_md,
                diff_item_css,
                diff_item_del,
            ]
            mock_repo_class.return_value = mock_repo

            files = scanner._get_changed_files(mock_repo, "old", "new")

        assert "src/Main.Py" in files
        assert "docs/Notes.md" in files
        assert "style.css" not in files

    def test_docstring_filter_uses_shared_predicate(self):
        """analyze_docstrings filters Python files via is_python_source (case-insensitive)."""
        assert scanner_module.is_python_source("SCRIPT.PY") is True
        assert scanner_module.is_python_source("script.js") is False


class TestAnalysisConcurrency:
    """Per-file LLM analysis runs concurrently, not one round trip at a time."""

    @pytest.mark.asyncio
    async def test_analyze_code_quality_runs_files_concurrently(self, tmp_path):
        import asyncio
        from unittest.mock import MagicMock

        from drep.core.scanner import RepositoryScanner

        for name in ("a.py", "b.py", "c.py"):
            (tmp_path / name).write_text("x = 1\n")

        scanner = RepositoryScanner(MagicMock())

        in_flight = 0
        peak = 0

        async def slow_analyze(file_path, content, repo_id, commit_sha):
            nonlocal in_flight, peak
            in_flight += 1
            peak = max(peak, in_flight)
            await asyncio.sleep(0.01)
            in_flight -= 1
            return []

        scanner.code_analyzer = MagicMock()
        scanner.code_analyzer.analyze_file = slow_analyze

        await scanner.analyze_code_quality(
            repo_path=str(tmp_path),
            files=["a.py", "b.py", "c.py"],
            repo_id="o/r",
            commit_sha="sha",
        )

        assert peak > 1, "files were analyzed one at a time"

    @pytest.mark.asyncio
    async def test_one_failing_file_does_not_abort_the_rest(self, tmp_path):
        from unittest.mock import MagicMock

        from drep.core.scanner import RepositoryScanner
        from drep.models.findings import Finding

        for name in ("good.py", "bad.py"):
            (tmp_path / name).write_text("x = 1\n")

        scanner = RepositoryScanner(MagicMock())

        async def analyze(file_path, content, repo_id, commit_sha):
            if file_path == "bad.py":
                raise RuntimeError("LLM exploded")
            return [
                Finding(type="typo", severity="info", file_path=file_path, line=1, message="found")
            ]

        scanner.code_analyzer = MagicMock()
        scanner.code_analyzer.analyze_file = analyze

        result = await scanner.analyze_code_quality(
            repo_path=str(tmp_path),
            files=["good.py", "bad.py"],
            repo_id="o/r",
            commit_sha="sha",
        )

        assert [f.file_path for f in result.findings] == ["good.py"]
        # The failing file is named, not silently absent from the findings
        assert result.failed_files == ["bad.py"]


class TestFileTargetCaseInsensitivity:
    """The module promises "identical, case-insensitive decisions" - all of them.

    The suffix predicates lowercase, but is_ignored_dir compared raw, so a
    directory named VENV or .Git was descended into and analyzed. On a
    case-insensitive filesystem those are the same directory.
    """

    def test_ignored_dirs_are_matched_case_insensitively(self):
        from drep.core.file_targets import is_ignored_dir

        assert is_ignored_dir("venv")
        assert is_ignored_dir("VENV")
        assert is_ignored_dir("__PyCache__")
        assert is_ignored_dir(".GIT")

    def test_egg_info_suffix_is_matched_case_insensitively(self):
        from drep.core.file_targets import is_ignored_dir

        assert is_ignored_dir("drep.egg-info")
        assert is_ignored_dir("drep.EGG-INFO")

    def test_ordinary_directories_are_still_kept(self):
        from drep.core.file_targets import is_ignored_dir

        assert not is_ignored_dir("drep")
        assert not is_ignored_dir("tests")
        assert not is_ignored_dir("environment")


class TestSuffixExtraction:
    """A dot in a parent directory is not the file's extension."""

    def test_dotted_directory_does_not_create_a_suffix(self):
        from drep.core.file_targets import is_python_source, is_scan_target

        # rpartition over the whole string yielded '.v1/file' / '.py/README'
        assert not is_scan_target("src.v1/file")
        assert not is_python_source("docs.py/README")

    def test_real_extensions_still_match(self):
        from drep.core.file_targets import is_markdown, is_python_source

        assert is_python_source("src/main.py")
        assert is_python_source("SRC/MAIN.PY")
        assert is_markdown("docs/guide.md")


class TestExpandPaths:
    """One expansion routine for "files and/or directories" -> files.

    `drep check` and `drep lint-docs` each grew their own copy, and they
    disagreed: `drep check a.py .` handed a.py to the analyzer twice, which at
    reasoning-model prices is a duplicated LLM round-trip per overlap.
    """

    def test_a_file_named_twice_is_analyzed_once(self, tmp_path):
        from drep.core.file_targets import expand_paths, is_scan_target

        (tmp_path / "a.py").write_text("x = 1\n")
        (tmp_path / "b.py").write_text("x = 2\n")

        found = expand_paths([tmp_path / "a.py", tmp_path], is_scan_target)

        assert found == sorted({tmp_path / "a.py", tmp_path / "b.py"})

    def test_explicit_files_are_filtered_by_the_predicate(self, tmp_path):
        from drep.core.file_targets import expand_paths, is_scan_target

        (tmp_path / "a.py").write_text("x = 1\n")
        (tmp_path / "notes.rst").write_text("hi\n")

        found = expand_paths([tmp_path / "a.py", tmp_path / "notes.rst"], is_scan_target)

        assert found == [tmp_path / "a.py"]

    def test_directories_are_pruned(self, tmp_path):
        from drep.core.file_targets import expand_paths, is_scan_target

        (tmp_path / "a.py").write_text("x = 1\n")
        (tmp_path / "venv").mkdir()
        (tmp_path / "venv" / "vendored.py").write_text("x = 3\n")

        assert expand_paths([tmp_path], is_scan_target) == [tmp_path / "a.py"]


class TestRegistryDrivenDiscovery:
    """File discovery asks the registry; it does not know language names."""

    @pytest.mark.parametrize(
        "path",
        ["src/main.py", "app/index.ts", "app/index.tsx", "cmd/server.go", "src/lib.rs", "a.js"],
    )
    def test_every_registered_language_is_a_scan_target(self, path):
        from drep.core.file_targets import is_scan_target

        assert is_scan_target(path)

    def test_markdown_is_still_a_scan_target(self):
        """Docs are not a code language, but the documentation analyzer wants them."""
        from drep.core.file_targets import is_scan_target

        assert is_scan_target("README.md")

    def test_unknown_types_are_not_targets(self):
        from drep.core.file_targets import is_scan_target

        assert not is_scan_target("notes.txt")
        assert not is_scan_target("image.png")
        assert not is_scan_target("Makefile")


class TestDocstringPassStaysPythonOnly:
    """ast.parse must never be handed a Go file.

    The docstring pass filters with is_python_source independently of the code
    analyzer's predicate, so widening code-quality coverage cannot reach it.
    This test is the guard on that property.
    """

    @pytest.mark.parametrize("path", ["app/index.ts", "cmd/server.go", "src/lib.rs", "a.js"])
    def test_other_languages_are_not_python_source(self, path):
        from drep.core.file_targets import is_python_source

        assert not is_python_source(path)

    def test_python_still_is(self):
        from drep.core.file_targets import is_python_source

        assert is_python_source("src/main.py")
