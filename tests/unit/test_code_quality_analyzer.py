"""Unit tests for CodeQualityAnalyzer."""

import pytest

from drep.code_quality.analyzer import MAX_FILE_SIZE, CodeQualityAnalyzer


@pytest.fixture
def analyzer(mock_llm_client):
    """Create analyzer with mocked LLM client."""
    return CodeQualityAnalyzer(mock_llm_client)


@pytest.mark.asyncio
async def test_analyze_file_success(analyzer, mock_llm_client):
    """Test successful file analysis."""
    # Mock LLM response
    mock_llm_client.analyze_code_json.return_value = {
        "issues": [
            {
                "line": 42,
                "severity": "high",
                "category": "bug",
                "message": "Potential null pointer dereference",
                "suggestion": "Add null check before dereferencing",
                "code_snippet": "user.name.upper()",
            }
        ],
        "summary": "Found 1 high-severity bug",
    }

    # Analyze file
    findings = await analyzer.analyze_file(
        file_path="test.py",
        content="def test():\n    user.name.upper()",
        repo_id="test-repo",
        commit_sha="abc123",
    )

    # Verify LLM was called
    mock_llm_client.analyze_code_json.assert_called_once()

    # Verify findings
    assert len(findings) == 1
    assert findings[0].type == "bug"
    assert findings[0].severity == "error"  # high -> error
    assert findings[0].line == 42
    assert findings[0].file_path == "test.py"
    assert "null pointer" in findings[0].message.lower()


@pytest.mark.asyncio
async def test_analyze_file_no_issues(analyzer, mock_llm_client):
    """Test file analysis with no issues found."""
    # Mock LLM response with no issues
    mock_llm_client.analyze_code_json.return_value = {
        "issues": [],
        "summary": "No issues found. Code looks good.",
    }

    # Analyze file
    findings = await analyzer.analyze_file(
        file_path="clean.py",
        content="def hello():\n    print('hello')",
        repo_id="test-repo",
        commit_sha="abc123",
    )

    # Verify no findings
    assert len(findings) == 0
    mock_llm_client.analyze_code_json.assert_called_once()


@pytest.mark.asyncio
async def test_analyze_file_multiple_issues(analyzer, mock_llm_client):
    """Test file analysis with multiple issues."""
    # Mock LLM response with multiple issues
    mock_llm_client.analyze_code_json.return_value = {
        "issues": [
            {
                "line": 10,
                "severity": "critical",
                "category": "security",
                "message": "SQL injection vulnerability",
                "suggestion": "Use parameterized queries",
                "code_snippet": "cursor.execute(f'SELECT * FROM users WHERE id={user_id}')",
            },
            {
                "line": 25,
                "severity": "medium",
                "category": "best-practice",
                "message": "Missing docstring",
                "suggestion": "Add docstring describing function purpose",
                "code_snippet": "def process_data(data):",
            },
            {
                "line": 50,
                "severity": "low",
                "category": "style",
                "message": "Variable name not descriptive",
                "suggestion": "Use more descriptive variable name",
                "code_snippet": "x = get_data()",
            },
        ],
        "summary": "Found 3 issues",
    }

    # Analyze file
    findings = await analyzer.analyze_file(
        file_path="issues.py",
        content="# code with issues",
        repo_id="test-repo",
        commit_sha="abc123",
    )

    # Verify findings
    assert len(findings) == 3

    # Check severity mapping
    assert findings[0].severity == "error"  # critical -> error
    assert findings[1].severity == "warning"  # medium -> warning
    assert findings[2].severity == "info"  # low -> info

    # Check categories preserved
    assert findings[0].type == "security"
    assert findings[1].type == "best-practice"
    assert findings[2].type == "style"


@pytest.mark.asyncio
async def test_analyze_file_too_large(analyzer, mock_llm_client):
    """Test that large files are skipped."""
    # Create file larger than MAX_FILE_SIZE
    large_content = "x = 1\n" * (MAX_FILE_SIZE // 6 + 1000)

    # Analyze file
    findings = await analyzer.analyze_file(
        file_path="large.py", content=large_content, repo_id="test-repo", commit_sha="abc123"
    )

    # Should skip without calling LLM
    assert len(findings) == 0
    mock_llm_client.analyze_code_json.assert_not_called()


@pytest.mark.asyncio
async def test_analyze_file_empty(analyzer, mock_llm_client):
    """Test that empty files are skipped."""
    # Analyze empty file
    findings = await analyzer.analyze_file(
        file_path="empty.py", content="", repo_id="test-repo", commit_sha="abc123"
    )

    # Should skip without calling LLM
    assert len(findings) == 0
    mock_llm_client.analyze_code_json.assert_not_called()


@pytest.mark.asyncio
async def test_analyze_file_whitespace_only(analyzer, mock_llm_client):
    """Test that whitespace-only files are skipped."""
    # Analyze whitespace-only file
    findings = await analyzer.analyze_file(
        file_path="whitespace.py",
        content="   \n\n\t\t\n   ",
        repo_id="test-repo",
        commit_sha="abc123",
    )

    # Should skip without calling LLM
    assert len(findings) == 0
    mock_llm_client.analyze_code_json.assert_not_called()


@pytest.mark.asyncio
async def test_analyze_file_json_parse_error(analyzer, mock_llm_client):
    """An unparseable response means the file went unanalyzed - say so."""
    # Mock LLM to raise ValueError (JSON parsing failure)
    mock_llm_client.analyze_code_json.side_effect = ValueError("Failed to parse JSON")

    with pytest.raises(ValueError, match="Failed to parse JSON"):
        await analyzer.analyze_file(
            file_path="test.py",
            content="def test(): pass",
            repo_id="test-repo",
            commit_sha="abc123",
        )


@pytest.mark.asyncio
async def test_analyze_file_llm_request_error(analyzer, mock_llm_client):
    """LLM transport failures must propagate, never read as a clean file.

    The caller (RepositoryScanner) counts the file as failed; swallowing here
    would make an unreachable endpoint indistinguishable from clean code.
    """
    # Mock LLM to raise exception
    mock_llm_client.analyze_code_json.side_effect = Exception("LLM request failed")

    with pytest.raises(Exception, match="LLM request failed"):
        await analyzer.analyze_file(
            file_path="test.py",
            content="def test(): pass",
            repo_id="test-repo",
            commit_sha="abc123",
        )


@pytest.mark.asyncio
async def test_analyze_file_passes_correct_parameters(analyzer, mock_llm_client):
    """Test that correct parameters are passed to LLM client."""
    mock_llm_client.analyze_code_json.return_value = {"issues": [], "summary": "Clean"}

    await analyzer.analyze_file(
        file_path="test.py", content="def test(): pass", repo_id="my-repo", commit_sha="commit123"
    )

    # Verify call parameters
    call_args = mock_llm_client.analyze_code_json.call_args
    assert call_args.kwargs["code"] == "def test(): pass"
    assert call_args.kwargs["repo_id"] == "my-repo"
    assert call_args.kwargs["commit_sha"] == "commit123"
    assert "system_prompt" in call_args.kwargs
    assert "schema" in call_args.kwargs


@pytest.mark.asyncio
async def test_analyze_files_multiple(analyzer, mock_llm_client):
    """Test analyzing multiple files."""
    # Mock LLM to return different issues for each file
    mock_llm_client.analyze_code_json.side_effect = [
        {
            "issues": [
                {
                    "line": 1,
                    "severity": "high",
                    "category": "bug",
                    "message": "Bug in file 1",
                    "suggestion": "Fix it",
                    "code_snippet": "code1",
                }
            ],
            "summary": "1 issue in file 1",
        },
        {
            "issues": [
                {
                    "line": 2,
                    "severity": "medium",
                    "category": "style",
                    "message": "Style issue in file 2",
                    "suggestion": "Fix style",
                    "code_snippet": "code2",
                }
            ],
            "summary": "1 issue in file 2",
        },
    ]

    # Analyze multiple files
    files = [("file1.py", "content1"), ("file2.py", "content2")]

    findings = await analyzer.analyze_files(files, repo_id="test-repo", commit_sha="abc123")

    # Verify combined findings from both files
    assert len(findings) == 2
    assert findings[0].file_path == "file1.py"
    assert findings[1].file_path == "file2.py"
    assert mock_llm_client.analyze_code_json.call_count == 2


@pytest.mark.asyncio
async def test_analyze_files_propagates_failure(analyzer, mock_llm_client):
    """A failure mid-batch aborts rather than returning partial results as complete."""
    # Mock LLM: first succeeds, second fails, third succeeds
    mock_llm_client.analyze_code_json.side_effect = [
        {
            "issues": [
                {
                    "line": 1,
                    "severity": "high",
                    "category": "bug",
                    "message": "Bug",
                    "suggestion": "Fix",
                    "code_snippet": "code",
                }
            ],
            "summary": "1 issue",
        },
        Exception("LLM failed"),
        {"issues": [], "summary": "Clean"},
    ]

    files = [("file1.py", "content1"), ("file2.py", "content2"), ("file3.py", "content3")]

    with pytest.raises(Exception, match="LLM failed"):
        await analyzer.analyze_files(files, repo_id="test-repo", commit_sha="abc123")


@pytest.mark.asyncio
async def test_analyze_files_empty_list(analyzer, mock_llm_client):
    """Test analyzing empty file list."""
    findings = await analyzer.analyze_files([], repo_id="test-repo", commit_sha="abc123")

    assert len(findings) == 0
    mock_llm_client.analyze_code_json.assert_not_called()


def test_is_supported_file_python(analyzer):
    """Test that Python files are supported."""
    assert analyzer.is_supported_file("test.py") is True
    assert analyzer.is_supported_file("src/module.py") is True
    assert analyzer.is_supported_file("/abs/path/to/file.py") is True


def test_is_supported_file_unclaimed_types(analyzer):
    """Types no registered language claims are not analyzed.

    .js moved to the supported side when the language registry landed; these
    are the ones still outside it. Markdown is deliberately excluded - it goes
    to the documentation analyzer, not a code analyzer.
    """
    assert analyzer.is_supported_file("README.md") is False
    assert analyzer.is_supported_file("config.json") is False
    assert analyzer.is_supported_file("test.txt") is False
    assert analyzer.is_supported_file("Makefile") is False


def test_is_supported_file_no_extension(analyzer):
    """Test files without extensions are not supported."""
    assert analyzer.is_supported_file("test") is False
    assert analyzer.is_supported_file("/path/to/file") is False


@pytest.mark.asyncio
async def test_analyze_file_at_size_limit(analyzer, mock_llm_client):
    """Test file exactly at size limit is analyzed."""
    # Create file exactly at MAX_FILE_SIZE
    content = "x" * MAX_FILE_SIZE

    mock_llm_client.analyze_code_json.return_value = {"issues": [], "summary": "Clean"}

    await analyzer.analyze_file(
        file_path="exactly_max.py", content=content, repo_id="test-repo", commit_sha="abc123"
    )

    # Should be analyzed (at limit is OK, over limit is rejected)
    mock_llm_client.analyze_code_json.assert_called_once()


@pytest.mark.asyncio
async def test_analyze_file_severity_edge_cases(analyzer, mock_llm_client):
    """Test all severity level mappings."""
    # Test all LLM severity levels
    mock_llm_client.analyze_code_json.return_value = {
        "issues": [
            {
                "line": 1,
                "severity": "critical",
                "category": "bug",
                "message": "Critical",
                "suggestion": "Fix",
                "code_snippet": "code",
            },
            {
                "line": 2,
                "severity": "high",
                "category": "bug",
                "message": "High",
                "suggestion": "Fix",
                "code_snippet": "code",
            },
            {
                "line": 3,
                "severity": "medium",
                "category": "bug",
                "message": "Medium",
                "suggestion": "Fix",
                "code_snippet": "code",
            },
            {
                "line": 4,
                "severity": "low",
                "category": "bug",
                "message": "Low",
                "suggestion": "Fix",
                "code_snippet": "code",
            },
            {
                "line": 5,
                "severity": "info",
                "category": "bug",
                "message": "Info",
                "suggestion": "Fix",
                "code_snippet": "code",
            },
        ],
        "summary": "All severity levels",
    }

    findings = await analyzer.analyze_file(
        file_path="test.py", content="code", repo_id="test-repo", commit_sha="abc123"
    )

    # Verify severity mappings
    assert findings[0].severity == "error"  # critical -> error
    assert findings[1].severity == "error"  # high -> error
    assert findings[2].severity == "warning"  # medium -> warning
    assert findings[3].severity == "info"  # low -> info
    assert findings[4].severity == "info"  # info -> info


class TestLanguageAwareAnalysis:
    """The analyzer reads any registered language, with the right prompt."""

    @pytest.mark.asyncio
    async def test_supports_every_registered_language(self, analyzer):
        for path in ("a.py", "a.ts", "a.tsx", "a.js", "a.go", "a.rs"):
            assert analyzer.is_supported_file(path)

    @pytest.mark.asyncio
    async def test_rejects_files_no_language_claims(self, analyzer):
        assert not analyzer.is_supported_file("style.css")
        assert not analyzer.is_supported_file("README.md")

    @pytest.mark.asyncio
    async def test_prompt_matches_the_file_language(self, analyzer, mock_llm_client):
        mock_llm_client.analyze_code_json.return_value = {"issues": [], "summary": "Clean"}

        await analyzer.analyze_file(
            file_path="cmd/server.go", content="package main\n", repo_id="r", commit_sha="s"
        )

        prompt = mock_llm_client.analyze_code_json.call_args.kwargs["system_prompt"]
        assert "Go" in prompt
        # The Python rubric must not follow a Go file around
        assert "PEP 8" not in prompt

    @pytest.mark.asyncio
    async def test_python_still_gets_its_own_conventions(self, analyzer, mock_llm_client):
        mock_llm_client.analyze_code_json.return_value = {"issues": [], "summary": "Clean"}

        await analyzer.analyze_file(
            file_path="main.py", content="x = 1\n", repo_id="r", commit_sha="s"
        )

        prompt = mock_llm_client.analyze_code_json.call_args.kwargs["system_prompt"]
        assert "PEP 8" in prompt

    @pytest.mark.asyncio
    async def test_analyzer_key_is_per_language(self, analyzer, mock_llm_client):
        """Cache and metrics segment by language, so a .go response can never
        be served from a .py cache entry."""
        mock_llm_client.analyze_code_json.return_value = {"issues": [], "summary": "Clean"}

        await analyzer.analyze_file(
            file_path="src/lib.rs", content="fn main() {}\n", repo_id="r", commit_sha="s"
        )

        assert mock_llm_client.analyze_code_json.call_args.kwargs["analyzer"] == "code_quality_rust"

    @pytest.mark.asyncio
    async def test_unknown_language_is_not_analyzed(self, analyzer, mock_llm_client):
        findings = await analyzer.analyze_file(
            file_path="style.css", content="body {}\n", repo_id="r", commit_sha="s"
        )

        assert findings == []
        mock_llm_client.analyze_code_json.assert_not_called()
