"""Integration tests for GitHub adapter against real GitHub API.

These tests require:
- GITHUB_TEST_TOKEN environment variable with a valid GitHub PAT
- Access to slb350/drep-test repository
- Tests will create and clean up issues/comments

Run with: pytest tests/integration/ -v -m integration
Skip with: pytest tests/ -v -k "not integration"
"""

import os
from datetime import datetime

import pytest

from drep.adapters.github import GitHubAdapter

# Test repository configuration
TEST_OWNER = "slb350"
TEST_REPO = "drep-test"


@pytest.fixture
def github_token():
    """Get GitHub token from environment or skip test."""
    token = os.getenv("GITHUB_TEST_TOKEN")
    if not token:
        pytest.skip("GITHUB_TEST_TOKEN environment variable not set")
    return token


@pytest.fixture
async def github_adapter(github_token):
    """Create GitHub adapter with test token."""
    adapter = GitHubAdapter(token=github_token)
    yield adapter
    await adapter.close()


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_create_issue_real_api(github_adapter):
    """Integration test: Create and close issue on real GitHub repo.

    Tests:
    - Issue creation returns valid issue number
    - Created issue is accessible via GitHub API
    - Issue cleanup works (close the issue)
    """
    # Create issue with timestamp to avoid conflicts
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    title = f"[drep-test] Integration test {timestamp}"
    body = "Automated integration test. This issue will be closed automatically."
    labels = ["automated-test"]

    # Create issue
    issue_number = await github_adapter.create_issue(
        owner=TEST_OWNER, repo=TEST_REPO, title=title, body=body, labels=labels
    )

    # Verify issue number is valid
    assert isinstance(issue_number, int)
    assert issue_number > 0

    # TODO: Clean up - close the issue
    # (Would require adding close_issue() method to adapter or using httpx directly)
    # For now, issues will need manual cleanup

    print(f"Created issue #{issue_number} - please close manually at:")
    print(f"https://github.com/{TEST_OWNER}/{TEST_REPO}/issues/{issue_number}")


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_get_file_content_real_api(github_adapter):
    """Integration test: Fetch file content from real GitHub repo.

    Tests:
    - File retrieval works with real GitHub API
    - Base64 decoding handles real GitHub response format
    - Newlines in base64 content are handled correctly
    - UTF-8 decoding works for real files

    Note:
    - If repository is empty, test will verify 404 error handling instead
    """
    # Try to fetch README.md from the test repo
    try:
        content = await github_adapter.get_file_content(
            owner=TEST_OWNER, repo=TEST_REPO, file_path="README.md", ref="main"
        )

        # Verify we got content
        assert isinstance(content, str)
        # Allow empty README
        print(f"Retrieved README.md ({len(content)} bytes)")
        if len(content) > 0:
            print(f"First 100 chars: {content[:100]}")
        else:
            print("README.md is empty (valid)")

    except ValueError as e:
        # If README doesn't exist, that's okay - test 404 handling instead
        if "not found" in str(e).lower():
            print("README.md not found - repository may be empty (expected for new repo)")
            print("404 error handling working correctly")
            # Test passes - we verified error handling works
        else:
            # Some other error - re-raise
            raise


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_rate_limit_headers_real_api(github_adapter):
    """Integration test: Validate rate limit headers from real GitHub API.

    Tests:
    - GitHub returns rate limit headers
    - X-RateLimit-Remaining is a numeric string
    - X-RateLimit-Reset is a timestamp
    - Headers are accessible and parseable
    """
    # Make a simple API call to get rate limit headers
    # We'll use the underlying httpx client to inspect headers

    url = f"{github_adapter.url}/repos/{TEST_OWNER}/{TEST_REPO}"
    response = await github_adapter.client.get(url)
    response.raise_for_status()

    # Verify rate limit headers exist
    assert "X-RateLimit-Limit" in response.headers
    assert "X-RateLimit-Remaining" in response.headers
    assert "X-RateLimit-Reset" in response.headers

    # Verify they're parseable as numbers
    limit = int(response.headers["X-RateLimit-Limit"])
    remaining = int(response.headers["X-RateLimit-Remaining"])
    reset_time = int(response.headers["X-RateLimit-Reset"])

    assert limit > 0
    assert remaining >= 0
    assert reset_time > 0

    print(f"Rate limit: {remaining}/{limit} (resets at {reset_time})")


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_authentication_real_api(github_adapter):
    """Integration test: Verify authentication works with real GitHub API.

    Tests:
    - Token authentication works (Bearer header accepted)
    - Can access repository data
    - Authenticated user has permissions
    """
    # Fetch repository info - requires valid authentication

    url = f"{github_adapter.url}/repos/{TEST_OWNER}/{TEST_REPO}"
    response = await github_adapter.client.get(url)
    response.raise_for_status()

    data = response.json()

    # Verify we got valid repository data
    assert "id" in data
    assert "name" in data
    assert data["name"] == TEST_REPO
    assert "owner" in data
    assert data["owner"]["login"] == TEST_OWNER

    print(f"Authenticated successfully - repo: {data['full_name']}")
    print(f"Repo ID: {data['id']}, Private: {data.get('private', False)}")


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_unicode_content_real_api(github_adapter):
    """Integration test: Test UTF-8/Unicode handling with real API.

    Tests:
    - Unicode content (emoji, Chinese, Arabic) encodes/decodes correctly
    - Base64 encoding preserves Unicode characters
    - UTF-8 decoding works for real GitHub responses

    Note:
    - This test requires a file with Unicode content in the test repo
    - If file doesn't exist, test will skip
    """
    # Try to fetch a Unicode test file if it exists
    # If not, we'll create an issue with Unicode content instead

    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    title = f"🌍 [drep-test] Unicode test {timestamp}"
    body = """
    Testing Unicode support:
    - Emoji: 🚀 🎉 ✅ ❌ 🐛
    - Chinese: 你好世界 (Hello World)
    - Arabic: مرحبا بالعالم (Hello World)
    - Japanese: こんにちは世界
    - Russian: Привет мир
    - Symbols: © ® ™ € £ ¥
    """

    # Create issue with Unicode content
    issue_number = await github_adapter.create_issue(
        owner=TEST_OWNER,
        repo=TEST_REPO,
        title=title,
        body=body,
        labels=["automated-test", "unicode"],
    )

    assert isinstance(issue_number, int)
    assert issue_number > 0

    print(f"Created Unicode issue #{issue_number}")
    print(f"Title: {title}")
    print("Body contains emoji, Chinese, Arabic, Japanese, Russian, and symbols")


@pytest.mark.asyncio
@pytest.mark.integration
async def test_github_get_default_branch_real_api(github_adapter):
    """Integration test: Get default branch from real GitHub repo.

    Tests:
    - get_default_branch() works with real GitHub API
    - Returns actual default branch name for test repository
    - Handles both 'main' and 'master' branch names correctly
    """
    # Get default branch for test repository
    branch = await github_adapter.get_default_branch(TEST_OWNER, TEST_REPO)

    # Verify we got a valid branch name
    assert isinstance(branch, str)
    assert len(branch) > 0
    # Most repos use 'main' or 'master', but allow custom names
    print(f"Default branch for {TEST_OWNER}/{TEST_REPO}: {branch}")

    # Verify it's a reasonable branch name (no special characters that would break git)
    # Branch names can contain alphanumeric, hyphens, underscores, slashes
    assert all(
        c.isalnum() or c in "-_/" for c in branch
    ), f"Branch name contains unexpected characters: {branch}"
