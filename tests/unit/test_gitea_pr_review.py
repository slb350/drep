"""GiteaAdapter PR review tests: get_pr, diffs, comments, anchored inline comments."""

import httpx
import pytest
import respx

from drep.adapters.base import ReviewAnchor
from drep.adapters.gitea import GiteaAdapter

# ===== PR Review Tests =====


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_success():
    """Test get_pr() successfully fetches PR details."""

    # Mock PR response
    pr_data = {
        "number": 42,
        "title": "Add feature X",
        "body": "This PR adds feature X",
        "state": "open",
        "base": {"ref": "main"},
        "head": {"ref": "feature-x", "sha": "abc123def456"},
        "user": {"login": "steve"},
    }

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42").mock(
        return_value=httpx.Response(200, json=pr_data)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        result = await adapter.get_pr("steve", "drep", 42)
        assert result["number"] == 42
        assert result["title"] == "Add feature X"
        assert result["state"] == "open"
        assert result["head"]["sha"] == "abc123def456"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_not_found():
    """Test get_pr() raises ValueError for non-existent PR."""

    # Mock 404 response
    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/999").mock(
        return_value=httpx.Response(404, json={"message": "Not Found"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Pull request #999 not found"):
            await adapter.get_pr("steve", "drep", 999)
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_success():
    """Test get_pr_diff() successfully fetches PR diff."""

    diff_content = """diff --git a/src/module.py b/src/module.py
index abc123..def456 100644
--- a/src/module.py
+++ b/src/module.py
@@ -10,7 +10,9 @@ def calculate(x, y):
     \"\"\"Calculate sum.\"\"\"
-    return x + y
+    result = x + y
+    logger.info(f"Calculated: {result}")
+    return result
"""

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42.diff").mock(
        return_value=httpx.Response(200, text=diff_content)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        diff = await adapter.get_pr_diff("steve", "drep", 42)
        assert "diff --git" in diff
        assert "src/module.py" in diff
        assert "+    result = x + y" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_get_pr_diff_large():
    """Test get_pr_diff() handles large diffs (no size limit at this layer)."""

    # Create a large diff (> 100KB)
    large_diff = "diff --git a/file.py b/file.py\n" + ("+" + "x" * 1000 + "\n") * 200

    respx.get("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42.diff").mock(
        return_value=httpx.Response(200, text=large_diff)
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        diff = await adapter.get_pr_diff("steve", "drep", 42)
        assert len(diff) > 100000
        assert "diff --git" in diff
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_success():
    """Test create_pr_comment() successfully posts general comment."""

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues/42/comments").mock(
        return_value=httpx.Response(201, json={"id": 123, "body": "Test comment"})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Should not raise
        await adapter.create_pr_comment("steve", "drep", 42, "Test comment")
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_comment_sends_correct_payload():
    """Test create_pr_comment() sends correct JSON payload."""

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 123})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/issues/42/comments").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_pr_comment("steve", "drep", 42, "Review summary comment")

        # Verify payload
        assert request_data["payload"]["body"] == "Review summary comment"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_success():
    """Test create_pr_review_comment() successfully posts inline comment."""

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        return_value=httpx.Response(201, json={"id": 456})
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        # Should not raise
        await adapter.create_pr_review_comment(
            anchor=ReviewAnchor(
                owner="steve", repo="drep", pr_number=42, commit_sha="abc123def456"
            ),
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_sends_correct_payload():
    """Test create_pr_review_comment() sends correct JSON payload."""
    from drep.adapters.gitea import REVIEW_BODY_PLACEHOLDER

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 456})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_pr_review_comment(
            anchor=ReviewAnchor(
                owner="steve", repo="drep", pr_number=42, commit_sha="abc123def456"
            ),
            file_path="src/module.py",
            line=15,
            body="Consider adding error handling here",
        )

        # Verify payload structure
        payload = request_data["payload"]
        assert payload["commit_id"] == "abc123def456"
        # Non-empty top-level body (some Gitea versions reject "") but the
        # finding stays only in the inline comment, not duplicated as a summary.
        assert payload["body"] == REVIEW_BODY_PLACEHOLDER
        assert len(payload["comments"]) == 1
        assert payload["comments"][0]["path"] == "src/module.py"
        assert payload["comments"][0]["new_position"] == 15
        assert payload["comments"][0]["body"] == "Consider adding error handling here"
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_always_sends_non_empty_body():
    """Regression for #11: review body is never empty, even for an empty comment.

    Some Gitea versions reject a review submission with an empty top-level body
    ("review event requires a body"). The placeholder is sent unconditionally,
    and the (possibly empty) inline comment text is preserved separately.
    """
    from drep.adapters.gitea import REVIEW_BODY_PLACEHOLDER

    request_data = {}

    def capture_request(request):
        import json

        request_data["payload"] = json.loads(request.content)
        return httpx.Response(201, json={"id": 456})

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        side_effect=capture_request
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        await adapter.create_pr_review_comment(
            anchor=ReviewAnchor(
                owner="steve", repo="drep", pr_number=42, commit_sha="abc123def456"
            ),
            file_path="src/module.py",
            line=15,
            body="",
        )

        payload = request_data["payload"]
        assert payload["body"] == REVIEW_BODY_PLACEHOLDER
        assert payload["comments"][0]["body"] == ""
    finally:
        await adapter.close()


@pytest.mark.asyncio
@respx.mock
async def test_create_pr_review_comment_error_handling():
    """Test create_pr_review_comment() raises ValueError on error."""

    respx.post("http://192.168.1.14:3000/api/v1/repos/steve/drep/pulls/42/reviews").mock(
        return_value=httpx.Response(403, text="Forbidden: Permission denied")
    )

    adapter = GiteaAdapter("http://192.168.1.14:3000", "token")

    try:
        with pytest.raises(ValueError, match="Failed to create review comment"):
            await adapter.create_pr_review_comment(
                anchor=ReviewAnchor(owner="steve", repo="drep", pr_number=42, commit_sha="abc123"),
                file_path="test.py",
                line=10,
                body="Comment",
            )
    finally:
        await adapter.close()
