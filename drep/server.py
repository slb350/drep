"""FastAPI server for webhook handling and health checks.

MVP scope:
- Health endpoint at /api/health
- Gitea webhook at /webhooks/gitea to trigger scans/reviews
"""

import asyncio
import hashlib
import hmac
import json
import logging
from collections.abc import Coroutine
from pathlib import Path
from typing import Any

import yaml
from fastapi import FastAPI, Header, HTTPException, Request

from drep import __version__
from drep.cli import _run_review, _run_scan
from drep.config import _substitute_tree, find_config_file

logger = logging.getLogger(__name__)

app = FastAPI(title="drep", version=__version__)

# Strong references to fire-and-forget tasks; without these the event loop may
# garbage-collect a task mid-execution (only a weak reference is held otherwise).
_BACKGROUND_TASKS: set[asyncio.Task[None]] = set()


def _spawn_background(coro: Coroutine[Any, Any, None]) -> None:
    """Schedule a background coroutine, keeping a strong reference until it finishes."""

    def _on_done(task: asyncio.Task[None]) -> None:
        _BACKGROUND_TASKS.discard(task)
        if task.cancelled():
            return
        exc = task.exception()
        if exc is not None:
            logger.error("Background task failed: %s", exc, exc_info=exc)

    task: asyncio.Task[None] = asyncio.create_task(coro)
    _BACKGROUND_TASKS.add(task)
    task.add_done_callback(_on_done)


@app.get("/api/health")
async def health() -> dict[str, Any]:
    """Simple health check endpoint."""
    return {"status": "ok"}


def _extract_owner_repo(payload: dict[str, Any]) -> tuple[str, str] | None:
    repo = payload.get("repository") or {}

    # Try full_name: "owner/repo"
    full_name = repo.get("full_name")
    if isinstance(full_name, str) and "/" in full_name:
        owner, name = full_name.split("/", 1)
        return owner, name

    # Try owner object with various keys
    owner_obj = repo.get("owner") or {}
    for key in ("login", "username", "name"):
        owner_val = owner_obj.get(key)
        if owner_val:
            break
    else:
        owner_val = None

    name_val = repo.get("name")
    if owner_val and name_val:
        return str(owner_val), str(name_val)

    return None


def _load_webhook_secret(config_path: str) -> str | None:
    """Read the optional webhook secret from the config file.

    Deliberately lightweight (raw YAML + env substitution, no full Config
    validation) so webhook handling does not depend on the rest of the
    config being valid; scan/review workflows validate the full config
    themselves.
    """
    try:
        with Path(config_path).open() as f:
            raw = yaml.safe_load(f)
        if not isinstance(raw, dict):
            return None
        resolved: dict[str, Any] = _substitute_tree(raw, set())
        secret = resolved.get("webhook_secret")
        return str(secret) if secret else None
    except Exception:
        logger.warning("Could not read webhook_secret from %s; treating as unset", config_path)
        return None


def _verify_signature(raw_body: bytes, signature: str | None, secret: str) -> bool:
    """Constant-time HMAC-SHA256 check of the X-Gitea-Signature header."""
    expected = hmac.new(secret.encode(), raw_body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(signature or "", expected)


@app.post("/webhooks/gitea")
async def webhook_gitea(
    request: Request, x_gitea_event: str | None = Header(default=None)
) -> dict[str, Any]:
    """Receive Gitea webhooks and trigger background scan/review."""
    raw_body = await request.body()

    # Discover config file (respects DREP_CONFIG env var via find_config_file)
    config_file = find_config_file(None)
    config_path = str(config_file)

    # Optional HMAC secret verification: when configured, requests without
    # a valid X-Gitea-Signature are rejected before any work is scheduled.
    secret = _load_webhook_secret(config_path)
    if secret is None:
        logger.warning(
            "webhook_secret is not configured; accepting unauthenticated webhook requests"
        )
    else:
        x_gitea_signature: str | None = request.headers.get("x-gitea-signature")
        if not _verify_signature(raw_body, x_gitea_signature, secret):
            raise HTTPException(status_code=403, detail="Invalid webhook signature")

    try:
        payload = json.loads(raw_body)
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid JSON: {e}") from e

    if not isinstance(payload, dict):
        raise HTTPException(status_code=400, detail="Webhook payload must be a JSON object")

    event = (x_gitea_event or "").lower()

    scheduled = False
    details: dict[str, Any] = {}

    owner_repo = _extract_owner_repo(payload)

    if event == "push" and owner_repo:
        owner, repo = owner_repo
        # Fire-and-forget scan (no metrics printing/progress)
        _spawn_background(
            _run_scan(owner, repo, config_path, show_metrics=False, show_progress=False)
        )
        scheduled = True
        details = {"action": "scan", "owner": owner, "repo": repo}

    elif event == "pull_request" and owner_repo:
        owner, repo = owner_repo
        pr = payload.get("pull_request") or {}
        pr_number = pr.get("number") or pr.get("index")
        if isinstance(pr_number, int):
            _spawn_background(_run_review(owner, repo, pr_number, config_path, post_comments=True))
            scheduled = True
            details = {"action": "review", "owner": owner, "repo": repo, "pr": pr_number}

    return {
        "received": True,
        "event": event or "unknown",
        "scheduled": scheduled,
        **({"details": details} if scheduled else {}),
    }
