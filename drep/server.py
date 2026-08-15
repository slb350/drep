"""FastAPI server for webhook handling and health checks.

MVP scope:
- Health endpoint at /api/health
- Gitea webhook at /webhooks/gitea to trigger scans/reviews
"""

import asyncio
from typing import Any

from fastapi import FastAPI, Header, HTTPException, Request

from drep.cli import _run_review, _run_scan
from drep.config import find_config_file

app = FastAPI(title="drep", version="0.1.0")

# Strong references to fire-and-forget tasks; without these the event loop may
# garbage-collect a task mid-execution (only a weak reference is held otherwise).
_BACKGROUND_TASKS: set[asyncio.Task[None]] = set()


def _spawn_background(coro: Any) -> None:
    """Schedule a background coroutine, keeping a strong reference until it finishes."""
    task: asyncio.Task[None] = asyncio.create_task(coro)
    _BACKGROUND_TASKS.add(task)
    task.add_done_callback(_BACKGROUND_TASKS.discard)


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


@app.post("/webhooks/gitea")
async def webhook_gitea(
    request: Request, x_gitea_event: str | None = Header(default=None)
) -> dict[str, Any]:
    """Receive Gitea webhooks and trigger background scan/review."""
    try:
        payload = await request.json()
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid JSON: {e}") from e

    event = (x_gitea_event or "").lower()

    # Discover config file (respects DREP_CONFIG env var via find_config_file)
    config_file = find_config_file(None)
    config_path = str(config_file)

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
