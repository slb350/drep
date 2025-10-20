"""FastAPI server for webhook handling and health checks.

MVP scope:
- Health endpoint at /api/health
- Gitea webhook at /webhooks/gitea to trigger scans/reviews
"""

import asyncio
import os
from typing import Any, Dict, Optional, Tuple

from fastapi import FastAPI, Header, HTTPException, Request

from drep.cli import _run_review, _run_scan

app = FastAPI(title="drep", version="0.1.0")


@app.get("/api/health")
async def health() -> Dict[str, Any]:
    """Simple health check endpoint."""
    return {"status": "ok"}


def _extract_owner_repo(payload: Dict[str, Any]) -> Optional[Tuple[str, str]]:
    repo = payload.get("repository") or {}

    # Try full_name: "owner/repo"
    full_name = repo.get("full_name")
    if isinstance(full_name, str) and "/" in full_name:
        owner, name = full_name.split("/", 1)
        return owner, name

    # Try owner object with various keys
    owner_obj = repo.get("owner") or {}
    for key in ("login", "username", "name"):
        owner = owner_obj.get(key)
        if owner:
            break
    else:
        owner = None

    name = repo.get("name")
    if owner and name:
        return str(owner), str(name)

    return None


@app.post("/webhooks/gitea")
async def webhook_gitea(
    request: Request, x_gitea_event: str | None = Header(default=None)
) -> Dict[str, Any]:
    """Receive Gitea webhooks and trigger background scan/review."""
    try:
        payload = await request.json()
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid JSON: {e}")

    event = (x_gitea_event or "").lower()
    config_path = os.environ.get("DREP_CONFIG", "config.yaml")

    scheduled = False
    details: Dict[str, Any] = {}

    owner_repo = _extract_owner_repo(payload)

    if event == "push" and owner_repo:
        owner, repo = owner_repo
        # Fire-and-forget scan (no metrics printing/progress)
        asyncio.create_task(
            _run_scan(owner, repo, config_path, show_metrics=False, show_progress=False)
        )
        scheduled = True
        details = {"action": "scan", "owner": owner, "repo": repo}

    elif event == "pull_request" and owner_repo:
        owner, repo = owner_repo
        pr = payload.get("pull_request") or {}
        pr_number = pr.get("number") or pr.get("index")
        if isinstance(pr_number, int):
            asyncio.create_task(
                _run_review(owner, repo, pr_number, config_path, post_comments=True)
            )
            scheduled = True
            details = {"action": "review", "owner": owner, "repo": repo, "pr": pr_number}

    return {
        "received": True,
        "event": event or "unknown",
        "scheduled": scheduled,
        **({"details": details} if scheduled else {}),
    }
