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


class TestWebhookHardening:
    """C18 + security: payload shape guard and HMAC signature verification."""

    def _post(self, client, payload, headers=None, raw=None):
        return client.post(
            "/webhooks/gitea",
            content=raw,
            json=None if raw is not None else payload,
            headers=headers or {"X-Gitea-Event": "push"},
        )

    def test_non_dict_json_returns_400(self):
        """List/scalar JSON payloads get 400, not AttributeError 500."""
        client = TestClient(app, raise_server_exceptions=False)
        resp = self._post(client, None, raw=b'["not", "a", "dict"]')
        assert resp.status_code == 400
        resp = self._post(client, None, raw=b'"scalar"')
        assert resp.status_code == 400

    def test_valid_signature_accepted(self, monkeypatch):
        import hashlib
        import hmac as hmac_mod

        monkeypatch.setattr("drep.server._load_webhook_secret", lambda p: "sekrit")
        monkeypatch.setattr("drep.server._run_scan", AsyncMock())

        body = b'{"repository": {"full_name": "owner/repo"}}'
        sig = hmac_mod.new(b"sekrit", body, hashlib.sha256).hexdigest()
        client = TestClient(app)
        resp = self._post(
            client,
            None,
            raw=body,
            headers={"X-Gitea-Event": "push", "X-Gitea-Signature": sig},
        )
        assert resp.status_code == 200
        assert resp.json()["scheduled"] is True

    def test_invalid_signature_rejected_403(self, monkeypatch):
        monkeypatch.setattr("drep.server._load_webhook_secret", lambda p: "sekrit")
        monkeypatch.setattr("drep.server._run_scan", AsyncMock())

        client = TestClient(app)
        resp = self._post(
            client,
            None,
            raw=b'{"repository": {"full_name": "owner/repo"}}',
            headers={"X-Gitea-Event": "push", "X-Gitea-Signature": "deadbeef"},
        )
        assert resp.status_code == 403

    def test_missing_signature_rejected_when_secret_configured(self, monkeypatch):
        monkeypatch.setattr("drep.server._load_webhook_secret", lambda p: "sekrit")
        monkeypatch.setattr("drep.server._run_scan", AsyncMock())

        client = TestClient(app)
        resp = self._post(client, None, raw=b'{"repository": {"full_name": "owner/repo"}}')
        assert resp.status_code == 403

    def test_no_secret_allows_anonymous(self, monkeypatch, caplog):
        monkeypatch.setattr("drep.server._load_webhook_secret", lambda p: None)
        monkeypatch.setattr("drep.server._run_scan", AsyncMock())

        with caplog.at_level("WARNING", logger="drep.server"):
            client = TestClient(app)
            resp = self._post(client, None, raw=b'{"repository": {"full_name": "owner/repo"}}')
        assert resp.status_code == 200
        assert any("webhook" in r.message.lower() for r in caplog.records)
