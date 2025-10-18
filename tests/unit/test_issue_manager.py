"""Tests for IssueManager."""

from unittest.mock import AsyncMock, MagicMock

import pytest

from drep.core.issue_manager import IssueManager
from drep.models.findings import Finding


@pytest.fixture
def mock_adapter():
    """Create a mock GiteaAdapter."""
    adapter = MagicMock()
    adapter.create_issue = AsyncMock(return_value=1)
    return adapter


@pytest.fixture
def mock_db():
    """Create a mock database session."""
    db = MagicMock()
    return db


@pytest.fixture
def issue_manager(mock_adapter, mock_db):
    """Create an IssueManager instance."""
    return IssueManager(mock_adapter, mock_db)


def test_issue_manager_initialization(mock_adapter, mock_db):
    """Test IssueManager can be instantiated."""
    manager = IssueManager(mock_adapter, mock_db)
    assert manager is not None


def test_issue_manager_stores_adapter_and_db(mock_adapter, mock_db):
    """Test IssueManager stores adapter and db_session."""
    manager = IssueManager(mock_adapter, mock_db)
    assert manager.adapter is mock_adapter
    assert manager.db is mock_db


@pytest.mark.asyncio
async def test_create_issues_for_findings_accepts_parameters(issue_manager):
    """Test create_issues_for_findings accepts correct parameters."""
    findings = []
    # Should not raise
    await issue_manager.create_issues_for_findings("owner", "repo", findings)


@pytest.mark.asyncio
async def test_create_issues_for_findings_with_empty_list(issue_manager):
    """Test create_issues_for_findings with empty list succeeds."""
    await issue_manager.create_issues_for_findings("owner", "repo", [])
    # Should complete without errors


def test_create_issues_for_findings_is_async(issue_manager):
    """Test create_issues_for_findings returns a coroutine."""
    import inspect

    result = issue_manager.create_issues_for_findings("owner", "repo", [])
    assert inspect.iscoroutine(result)
    # Clean up coroutine
    result.close()


# Tests for _generate_hash()


def test_generate_hash_returns_string(issue_manager):
    """Test _generate_hash returns a string."""
    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )
    hash_value = issue_manager._generate_hash(finding)
    assert isinstance(hash_value, str)


def test_generate_hash_same_finding_same_hash(issue_manager):
    """Test same finding produces same hash (consistency)."""
    finding1 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )
    finding2 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )

    hash1 = issue_manager._generate_hash(finding1)
    hash2 = issue_manager._generate_hash(finding2)

    assert hash1 == hash2


def test_generate_hash_different_line_different_hash(issue_manager):
    """Test different line produces different hash."""
    finding1 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )
    finding2 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=10,  # Different line
        message="Typo: 'teh'",
    )

    hash1 = issue_manager._generate_hash(finding1)
    hash2 = issue_manager._generate_hash(finding2)

    assert hash1 != hash2


def test_generate_hash_different_message_different_hash(issue_manager):
    """Test different message produces different hash."""
    finding1 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )
    finding2 = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'recieve'",  # Different message
    )

    hash1 = issue_manager._generate_hash(finding1)
    hash2 = issue_manager._generate_hash(finding2)

    assert hash1 != hash2


def test_generate_hash_is_md5_format(issue_manager):
    """Test hash is MD5 format (32 hex characters)."""
    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )
    hash_value = issue_manager._generate_hash(finding)

    # MD5 hex digest is 32 characters
    assert len(hash_value) == 32
    # All characters should be hex
    assert all(c in "0123456789abcdef" for c in hash_value)


# Tests for _generate_issue_body()


def test_generate_issue_body_has_required_sections(issue_manager):
    """Test issue body includes all required sections."""
    finding = Finding(
        type="typo",
        severity="info",
        file_path="src/main.py",
        line=42,
        message="Typo: 'teh'",
        suggestion="Did you mean 'the'?",
    )

    body = issue_manager._generate_issue_body(finding)

    # Check for required markdown sections
    assert "## Finding" in body
    assert "**Type:**" in body
    assert "**Severity:**" in body
    assert "**File:**" in body
    assert "**Line:**" in body
    assert "**Issue:**" in body
    assert "**Suggestion:**" in body


def test_generate_issue_body_includes_finding_details(issue_manager):
    """Test issue body includes correct finding details."""
    finding = Finding(
        type="pattern",
        severity="warning",
        file_path="docs/README.md",
        line=100,
        message="Pattern issue: double_space",
    )

    body = issue_manager._generate_issue_body(finding)

    # Verify all values are in the body
    assert "pattern" in body
    assert "warning" in body
    assert "docs/README.md" in body
    assert "100" in body
    assert "Pattern issue: double_space" in body


def test_generate_issue_body_with_suggestion(issue_manager):
    """Test issue body includes suggestion when present."""
    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
        suggestion="Did you mean 'the'?",
    )

    body = issue_manager._generate_issue_body(finding)

    assert "**Suggestion:**" in body
    assert "Did you mean 'the'?" in body


def test_generate_issue_body_without_suggestion(issue_manager):
    """Test issue body omits suggestion section when not present."""
    finding = Finding(
        type="pattern",
        severity="info",
        file_path="test.py",
        line=5,
        message="Pattern issue: trailing_whitespace",
        suggestion=None,  # No suggestion
    )

    body = issue_manager._generate_issue_body(finding)

    # Should not have suggestion section if suggestion is None
    # Count occurrences - should only appear in the drep attribution link, not as a field
    suggestion_count = body.count("**Suggestion:**")
    assert suggestion_count == 0


def test_generate_issue_body_has_footer(issue_manager):
    """Test issue body has drep attribution footer."""
    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )

    body = issue_manager._generate_issue_body(finding)

    # Check for footer with markdown separator and attribution
    assert "---" in body
    assert "drep" in body.lower()
    assert "github.com/stephenbrandon/drep" in body.lower()


# Tests for create_issues_for_findings()


@pytest.mark.asyncio
async def test_create_issues_for_findings_creates_issue(mock_adapter, mock_db):
    """Test creates issue via adapter when finding is new."""
    # Setup: No existing findings in cache
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    # Verify adapter.create_issue was called
    mock_adapter.create_issue.assert_called_once()
    call_args = mock_adapter.create_issue.call_args

    # Check title format: "[drep] type: file:line"
    assert call_args.kwargs["title"] == "[drep] typo: test.py:5"
    assert call_args.kwargs["owner"] == "owner"
    assert call_args.kwargs["repo"] == "repo"
    assert call_args.kwargs["labels"] == ["documentation", "automated"]


@pytest.mark.asyncio
async def test_create_issues_for_findings_skips_duplicate(mock_adapter, mock_db):
    """Test skips creating issue when finding hash already exists."""
    from drep.db.models import FindingCache

    # Setup: Finding already exists in cache
    existing_cache = FindingCache(
        owner="owner",
        repo="repo",
        file_path="test.py",
        finding_hash="existing_hash",
        issue_number=42,
    )
    mock_db.query.return_value.filter_by.return_value.first.return_value = existing_cache

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    # Verify adapter.create_issue was NOT called (deduplication)
    mock_adapter.create_issue.assert_not_called()


@pytest.mark.asyncio
async def test_create_issues_for_findings_caches_finding(mock_adapter, mock_db):
    """Test caches finding after creating issue."""
    # Setup: No existing findings
    mock_db.query.return_value.filter_by.return_value.first.return_value = None
    mock_adapter.create_issue.return_value = 123  # Issue number

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="typo",
        severity="info",
        file_path="src/main.py",
        line=42,
        message="Typo: 'teh'",
    )

    await manager.create_issues_for_findings("test_owner", "test_repo", [finding])

    # Verify cache entry was added to db
    mock_db.add.assert_called_once()
    cache_entry = mock_db.add.call_args[0][0]

    # Check cache entry fields
    assert cache_entry.owner == "test_owner"
    assert cache_entry.repo == "test_repo"
    assert cache_entry.file_path == "src/main.py"
    assert cache_entry.issue_number == 123
    assert len(cache_entry.finding_hash) == 32  # MD5 hash

    # Verify commit was called
    mock_db.commit.assert_called_once()


@pytest.mark.asyncio
async def test_create_issues_for_findings_handles_multiple_findings(mock_adapter, mock_db):
    """Test handles multiple findings correctly."""
    # Setup: No existing findings
    mock_db.query.return_value.filter_by.return_value.first.return_value = None
    mock_adapter.create_issue.side_effect = [1, 2, 3]  # Return different issue numbers

    manager = IssueManager(mock_adapter, mock_db)

    findings = [
        Finding(type="typo", severity="info", file_path="a.py", line=1, message="Typo 1"),
        Finding(type="typo", severity="info", file_path="b.py", line=2, message="Typo 2"),
        Finding(type="pattern", severity="info", file_path="c.py", line=3, message="Pattern 1"),
    ]

    await manager.create_issues_for_findings("owner", "repo", findings)

    # Should create 3 issues
    assert mock_adapter.create_issue.call_count == 3
    # Should add 3 cache entries
    assert mock_db.add.call_count == 3
    # Should commit 3 times
    assert mock_db.commit.call_count == 3


@pytest.mark.asyncio
async def test_create_issues_for_findings_issue_title_format(mock_adapter, mock_db):
    """Test issue title is formatted correctly."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="pattern",
        severity="warning",
        file_path="docs/README.md",
        line=100,
        message="Double space detected",
    )

    await manager.create_issues_for_findings("myorg", "myrepo", [finding])

    call_args = mock_adapter.create_issue.call_args
    # Title format: "[drep] type: file:line"
    assert call_args.kwargs["title"] == "[drep] pattern: docs/README.md:100"


@pytest.mark.asyncio
async def test_create_issues_for_findings_passes_issue_body(mock_adapter, mock_db):
    """Test passes correct issue body to adapter."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message="Typo: 'teh'",
        suggestion="Did you mean 'the'?",
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    call_args = mock_adapter.create_issue.call_args
    body = call_args.kwargs["body"]

    # Verify body has expected content
    assert "## Finding" in body
    assert "Typo: 'teh'" in body
    assert "Did you mean 'the'?" in body


# Edge case tests


@pytest.mark.asyncio
async def test_create_issues_for_findings_with_special_characters(mock_adapter, mock_db):
    """Test handles special characters in file path and message."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="typo",
        severity="info",
        file_path="docs/日本語/README.md",  # Unicode characters
        line=5,
        message="Typo: 'café' → 'cafe'",  # Accented characters
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    # Should not crash, should create issue
    mock_adapter.create_issue.assert_called_once()
    call_args = mock_adapter.create_issue.call_args

    # Verify special characters are preserved
    assert "日本語" in call_args.kwargs["title"]
    assert "café" in call_args.kwargs["body"]


@pytest.mark.asyncio
async def test_create_issues_for_findings_with_very_long_message(mock_adapter, mock_db):
    """Test handles very long messages without errors."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    # Create a very long message
    long_message = "This is a very long error message. " * 100

    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        message=long_message,
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    # Should not crash
    mock_adapter.create_issue.assert_called_once()
    call_args = mock_adapter.create_issue.call_args

    # Verify long message is in body
    assert long_message in call_args.kwargs["body"]


@pytest.mark.asyncio
async def test_create_issues_for_findings_api_error_handling(mock_adapter, mock_db):
    """Test handles API errors gracefully (continues with remaining findings)."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    # First call fails, second succeeds
    mock_adapter.create_issue.side_effect = [
        ValueError("API error"),
        42,  # Success
    ]

    manager = IssueManager(mock_adapter, mock_db)

    findings = [
        Finding(type="typo", severity="info", file_path="a.py", line=1, message="Typo 1"),
        Finding(type="typo", severity="info", file_path="b.py", line=2, message="Typo 2"),
    ]

    # Should raise the ValueError from the first finding
    with pytest.raises(ValueError, match="API error"):
        await manager.create_issues_for_findings("owner", "repo", findings)

    # First call was attempted
    assert mock_adapter.create_issue.call_count == 1


@pytest.mark.asyncio
async def test_create_issues_for_findings_mixed_duplicates(mock_adapter, mock_db):
    """Test handles mix of new and duplicate findings."""

    # Setup: Second finding is duplicate
    def mock_query_side_effect(finding_hash):
        # Return existing cache only for specific hash
        if finding_hash == "duplicate_hash":
            from drep.db.models import FindingCache

            return FindingCache(
                owner="owner",
                repo="repo",
                file_path="b.py",
                finding_hash="duplicate_hash",
                issue_number=99,
            )
        return None

    # Mock the query chain
    query_mock = MagicMock()
    filter_by_mock = MagicMock()
    mock_db.query.return_value = query_mock
    query_mock.filter_by.return_value = filter_by_mock

    # Set up side effect for first() calls
    first_calls = []

    def first_side_effect():
        # First call (finding 1): no duplicate
        # Second call (finding 2): duplicate
        # Third call (finding 3): no duplicate
        result = [None, MagicMock(issue_number=99), None]
        idx = len(first_calls)
        first_calls.append(None)
        return result[idx] if idx < len(result) else None

    filter_by_mock.first.side_effect = first_side_effect
    mock_adapter.create_issue.side_effect = [1, 3]  # Issue numbers for non-duplicates

    manager = IssueManager(mock_adapter, mock_db)

    findings = [
        Finding(type="typo", severity="info", file_path="a.py", line=1, message="New 1"),
        Finding(type="typo", severity="info", file_path="b.py", line=2, message="Duplicate"),
        Finding(type="typo", severity="info", file_path="c.py", line=3, message="New 2"),
    ]

    await manager.create_issues_for_findings("owner", "repo", findings)

    # Should create 2 issues (1st and 3rd), skip 2nd
    assert mock_adapter.create_issue.call_count == 2
    assert mock_db.add.call_count == 2


@pytest.mark.asyncio
async def test_create_issues_for_findings_with_none_suggestion(mock_adapter, mock_db):
    """Test finding with None suggestion doesn't include suggestion in body."""
    mock_db.query.return_value.filter_by.return_value.first.return_value = None

    manager = IssueManager(mock_adapter, mock_db)

    finding = Finding(
        type="pattern",
        severity="info",
        file_path="test.py",
        line=5,
        message="Pattern issue",
        suggestion=None,
    )

    await manager.create_issues_for_findings("owner", "repo", [finding])

    call_args = mock_adapter.create_issue.call_args
    body = call_args.kwargs["body"]

    # Should not have Suggestion section
    assert "**Suggestion:**" not in body


@pytest.mark.asyncio
async def test_create_issues_for_findings_scoped_to_repository(mock_adapter, mock_db):
    """Test deduplication is scoped per repository (CRITICAL BUG FIX).

    The same finding hash in different repositories should create separate issues.
    This prevents cross-repository collision where repo B's findings are skipped
    because repo A already created an issue with the same hash.
    """
    # Setup: Finding exists in alice/repo-a but NOT in bob/repo-b
    from drep.db.models import FindingCache

    existing_cache = FindingCache(
        owner="alice",
        repo="repo-a",
        file_path="README.md",
        finding_hash="same_hash_123",
        issue_number=42,
    )

    # Mock query to return existing only when querying alice/repo-a
    query_mock = MagicMock()
    filter_by_mock = MagicMock()
    mock_db.query.return_value = query_mock
    query_mock.filter_by.return_value = filter_by_mock

    # Track filter_by calls to return appropriate results
    filter_by_calls = []

    def filter_by_side_effect(**kwargs):
        filter_by_calls.append(kwargs)
        # If querying alice/repo-a with this hash, return existing
        if (
            kwargs.get("owner") == "alice"
            and kwargs.get("repo") == "repo-a"
            and kwargs.get("finding_hash") == "same_hash_123"
        ):
            filter_by_mock.first.return_value = existing_cache
        else:
            # Otherwise return None (not found)
            filter_by_mock.first.return_value = None
        return filter_by_mock

    query_mock.filter_by = filter_by_side_effect
    mock_adapter.create_issue.return_value = 99

    manager = IssueManager(mock_adapter, mock_db)

    # Same finding (will generate same hash)
    finding = Finding(
        type="typo",
        severity="info",
        file_path="README.md",
        line=10,
        message="Typo: 'teh'",
    )

    # Scan bob/repo-b (different repository)
    await manager.create_issues_for_findings("bob", "repo-b", [finding])

    # CRITICAL: Should CREATE issue for bob/repo-b despite alice/repo-a having same hash
    mock_adapter.create_issue.assert_called_once()

    # Verify query included owner and repo (not just hash)
    assert len(filter_by_calls) > 0
    last_filter = filter_by_calls[-1]
    assert "owner" in last_filter, "Query must include owner to scope deduplication"
    assert "repo" in last_filter, "Query must include repo to scope deduplication"
    assert last_filter["owner"] == "bob"
    assert last_filter["repo"] == "repo-b"
