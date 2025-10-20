"""Integration-style test for Gitea inline comment fallback.

Simulates a 422 error for 'new_position' and success for 'position'.
"""

import httpx
import pytest

from drep.adapters.gitea import GiteaAdapter


@pytest.mark.asyncio
@pytest.mark.integration
async def test_create_pr_review_comment_fallback(monkeypatch):
    adapter = GiteaAdapter("http://gitea.local", "token")

    call_count = {"n": 0}

    async def fake_post(url, json):  # noqa: ARG001
        call_count["n"] += 1
        req = httpx.Request("POST", url)
        if call_count["n"] == 1:
            # First attempt fails (new_position)
            resp = httpx.Response(422, request=req, content=b'{"message":"bad field"}')
            raise httpx.HTTPStatusError("unprocessable", request=req, response=resp)
        # Second attempt succeeds (position)
        return httpx.Response(200, request=req, content=b"{}")

    monkeypatch.setattr(adapter.client, "post", fake_post)

    # Should not raise after fallback succeeds
    await adapter.create_pr_review_comment(
        owner="o",
        repo="r",
        pr_number=1,
        commit_sha="abc",
        file_path="file.py",
        line=10,
        body="test",
    )
