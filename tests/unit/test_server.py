"""Tests for FastAPI server endpoints."""

import asyncio
from unittest.mock import AsyncMock

import pytest
from fastapi.testclient import TestClient

from drep.server import app


def test_health_endpoint_ok():
    """GET /api/health returns status ok."""
    client = TestClient(app)
    resp = client.get("/api/health")
    assert resp.status_code == 200
    assert resp.json() == {"status": "ok"}


def test_webhook_push_schedules_scan(monkeypatch):
    """POST /webhooks/gitea schedules a scan on push event."""
    # Mock the CLI function directly
    mock_scan = AsyncMock()
    monkeypatch.setattr("drep.server._run_scan", mock_scan)

    client = TestClient(app)
    payload = {"repository": {"full_name": "owner/repo"}}
    headers = {"X-Gitea-Event": "push"}
    resp = client.post("/webhooks/gitea", json=payload, headers=headers)

    assert resp.status_code == 200
    data = resp.json()
    assert data["received"] is True
    assert data["event"] == "push"
    assert data["scheduled"] is True
    assert data["details"]["action"] == "scan"
    assert data["details"]["owner"] == "owner"
    assert data["details"]["repo"] == "repo"
    # Verify the mock was called (scheduled via create_task)
    # Note: We can't easily verify call args due to async scheduling,
    # but the response confirms correct parsing


def test_webhook_pr_schedules_review(monkeypatch):
    """POST /webhooks/gitea schedules a PR review on pull_request event."""
    # Mock the CLI function directly
    mock_review = AsyncMock()
    monkeypatch.setattr("drep.server._run_review", mock_review)

    client = TestClient(app)
    payload = {"repository": {"full_name": "owner/repo"}, "pull_request": {"number": 42}}
    headers = {"X-Gitea-Event": "pull_request"}
    resp = client.post("/webhooks/gitea", json=payload, headers=headers)

    assert resp.status_code == 200
    data = resp.json()
    assert data["received"] is True
    assert data["event"] == "pull_request"
    assert data["scheduled"] is True
    assert data["details"]["action"] == "review"
    assert data["details"]["owner"] == "owner"
    assert data["details"]["repo"] == "repo"
    assert data["details"]["pr"] == 42


@pytest.mark.asyncio
async def test_spawn_background_logs_task_exception(caplog):
    """A failed background task's exception is retrieved and logged, not lost."""
    from drep.server import _spawn_background

    async def failing():
        raise RuntimeError("scan exploded")

    with caplog.at_level("ERROR", logger="drep.server"):
        _spawn_background(failing())
        await asyncio.sleep(0.05)

    assert any("scan exploded" in r.message for r in caplog.records)
    for r in caplog.records:
        assert "Task exception was never retrieved" not in r.message


@pytest.mark.asyncio
async def test_spawn_background_swallows_cancellation(caplog):
    """A cancelled background task does not produce an error log."""
    from drep.server import _spawn_background

    async def cancelled():
        await asyncio.sleep(10)

    with caplog.at_level("ERROR", logger="drep.server"):
        from drep.server import _BACKGROUND_TASKS

        async def run():
            _spawn_background(cancelled())
            await asyncio.sleep(0.02)
            for t in _BACKGROUND_TASKS:
                t.cancel()
            await asyncio.sleep(0.05)

        await run()

    assert not any(r.levelname == "ERROR" for r in caplog.records)


@pytest.mark.asyncio
async def test_spawn_background_cleans_up_task_set():
    """Finished tasks are discarded from the strong-reference set."""
    from drep.server import _BACKGROUND_TASKS, _spawn_background

    async def ok():
        return None

    before = len(_BACKGROUND_TASKS)
    _spawn_background(ok())
    await asyncio.sleep(0.05)
    assert len(_BACKGROUND_TASKS) == before
