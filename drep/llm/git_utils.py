"""Git helpers for LLM cache keys.

Resolves the current commit SHA so cache keys can be invalidated when the
repository changes. Fails soft ("unknown") — a cache-key component should
never take the analysis down.
"""

import logging
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)


def get_current_commit_sha(repo_path: Path | None = None) -> str:
    """Get current git commit SHA for cache invalidation.

    This function is used to tie cached LLM responses to specific commits. When code
    changes (new commit), the cache is automatically invalidated, ensuring stale
    analysis results aren't reused.

    The function is designed to never fail - it returns "unknown" rather than raising
    exceptions, since cache invalidation is an optimization not a critical feature.

    Args:
        repo_path: Path to git repository (defaults to current directory).
                   Used when analyzing repos other than the one drep is running in.

    Returns:
        Commit SHA string (40-char hex), or "unknown" if:
        - Not in a git repository
        - Git command not available
        - Git command times out (shouldn't happen, but defensive)
        - Any other error occurs

    Note:
        Returns "unknown" for all errors rather than raising exceptions to ensure
        cache operations remain best-effort and don't break the main analysis flow.
    """
    try:
        # Determine which directory to run git command in
        cwd = repo_path or Path.cwd()

        # Execute git rev-parse HEAD to get current commit SHA
        # - capture_output=True: Capture stdout/stderr for logging
        # - text=True: Return strings instead of bytes
        # - timeout=5: Prevent hanging on network-mounted repos or weird git configs
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=5,
        )

        if result.returncode == 0:
            # Success - return the SHA (strip any trailing newline)
            return result.stdout.strip()
        # Not a git repository or git not available
        # This is expected when running outside a repo, so only warn-level logging
        logger.warning(f"Could not get commit SHA: {result.stderr}")
        return "unknown"

    except subprocess.TimeoutExpired:
        # Git command took >5 seconds (very rare, possibly network-mounted repo)
        logger.warning("Git command timed out")
        return "unknown"
    except FileNotFoundError:
        # git executable not in PATH (e.g., Windows without Git for Windows)
        logger.warning("Git not found in PATH")
        return "unknown"
    except Exception as e:
        # Catch-all for any other errors (permissions, etc.)
        logger.warning(f"Error getting commit SHA: {e}")
        return "unknown"
