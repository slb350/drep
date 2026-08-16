"""GitLabAdapter MR data access tests: get_pr and get_pr_diff."""

import httpx
import pytest
import respx

from drep.adapters.gitlab import GitLabAdapter

# ===== get_pr() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully retrieves merge request details."""

    mr_data = {
        "iid": 42,
        "title": "Test MR",
        "description": "Test description",
        "state": "opened",
        "source_branch": "feature",
        "target_branch": "main",
        "author": {"username": "testuser"},
        "diff_refs": {
            "base_sha": "abc123",
            "head_sha": "def456",
            "start_sha": "abc123",
        },
    }

    # Mock successful MR retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42").mock(
        return_value=httpx.Response(200, json=mr_data)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        result = await adapter.get_pr("owner", "repo", 42)
        assert result["iid"] == 42
        assert result["title"] == "Test MR"
        assert "diff_refs" in result
        assert result["diff_refs"]["base_sha"] == "abc123"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_404_error():
    """Test get_pr() raises ValueError for 404 (MR not found)."""

    # Mock 404 response
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/99").mock(
        return_value=httpx.Response(404, text="MR not found")
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"Merge request !99 not found"):
            await adapter.get_pr("owner", "repo", 99)
    finally:
        await adapter.close()


# ===== get_pr_diff() Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully retrieves and reconstructs diff."""

    diffs = [
        {
            "old_path": "file1.py",
            "new_path": "file1.py",
            "diff": "@@ -1,3 +1,4 @@\n import os\n+import sys\n",
        },
        {
            "old_path": "file2.py",
            "new_path": "file2.py",
            "diff": "@@ -1,2 +1,3 @@\n def test():\n+    pass\n",
        },
    ]

    # Mock successful diff retrieval
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=diffs)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)

        # Verify diff reconstruction
        assert "diff --git a/file1.py b/file1.py" in diff
        assert "import sys" in diff
        assert "diff --git a/file2.py b/file2.py" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_empty():
    """Test get_pr_diff() handles empty diff array."""

    # Mock empty diff
    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=[])
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        diff = await adapter.get_pr_diff("owner", "repo", 42)
        assert diff == ""
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_missing_old_path():
    """Test get_pr_diff() validates required 'old_path' field in diff objects."""

    # Mock diff missing 'old_path'
    diffs = [{"new_path": "file.py", "diff": "@@ -1 +1 @@\n"}]

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=diffs)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing required 'old_path' field"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_missing_new_path():
    """Test get_pr_diff() validates required 'new_path' field in diff objects."""

    # Mock diff missing 'new_path'
    diffs = [{"old_path": "file.py", "diff": "@@ -1 +1 @@\n"}]

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=diffs)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"missing required 'new_path' field"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_invalid_object_type():
    """Test get_pr_diff() validates diff objects are dicts, not strings/other types."""

    # Mock diff with string instead of dict
    diffs = ["invalid string object"]

    respx.get("https://gitlab.com/api/v4/projects/owner%2Frepo/merge_requests/42/diffs").mock(
        return_value=httpx.Response(200, json=diffs)
    )

    adapter = GitLabAdapter("glpat_token")

    try:
        with pytest.raises(ValueError, match=r"diff object at index 0 is not a dict"):
            await adapter.get_pr_diff("owner", "repo", 42)
    finally:
        await adapter.close()
