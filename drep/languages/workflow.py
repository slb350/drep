"""Run every applicable deterministic tool over a set of files.

Sits between the CLI workflows and the per-tool runner: groups files by
language, asks each language's tools to check the ones they own, and reports
what could not run.
"""

import asyncio
import logging
from pathlib import Path

from drep.languages.base import LanguageSupport, ToolSpec, registry
from drep.languages.runner import ToolOutcome, run_tool
from drep.models.findings import Finding

logger = logging.getLogger(__name__)


def relativise(file_path: str, root: Path) -> str:
    """Express a tool's path relative to the repo root where possible.

    ruff reports absolute paths, go vet relative ones. Left alone, one report
    mixes both and nothing groups by file. A path outside the root is returned
    unchanged - an absolute path is clearer than a ../../ chain.
    """
    candidate = Path(file_path)
    if not candidate.is_absolute():
        return file_path
    try:
        return str(candidate.relative_to(root))
    except ValueError:
        return file_path


def group_by_language(files: list[str]) -> dict[str, list[str]]:
    """Bucket repo-relative paths by the language that owns them.

    Files no language claims are dropped: markdown goes to the documentation
    analyzer, and everything else is not drep's business.
    """
    grouped: dict[str, list[str]] = {}
    for path in files:
        language = registry.detect(path)
        if language is not None:
            grouped.setdefault(language.name, []).append(path)
    return grouped


async def run_language_tools(root: Path, files: list[str]) -> tuple[list[Finding], list[str]]:
    """Run each language's configured tools over the files they own.

    Args:
        root: Repository root - tool discovery, config detection and the
            working directory are all relative to it.
        files: Repo-relative paths.

    Returns:
        (findings, unavailable) where `findings` are deterministic and safe to
        gate on, and `unavailable` names the tools that should have run and
        could not. An unavailable tool is reported rather than ignored: it is
        the same "not run is not clean" rule the LLM path follows.
    """
    grouped = group_by_language(files)
    if not grouped:
        return [], []

    jobs: list[tuple[LanguageSupport, ToolSpec, list[str]]] = [
        (registry.get(name), spec, paths)
        for name, paths in grouped.items()
        for spec in registry.get(name).tools
    ]
    if not jobs:
        return [], []

    # Tools are independent processes; running them concurrently means a repo
    # with four languages waits for the slowest tool, not the sum of them.
    outcomes: list[ToolOutcome] = list(
        await asyncio.gather(*(run_tool(spec, root, paths) for _, spec, paths in jobs))
    )

    findings: list[Finding] = []
    unavailable: list[str] = []
    for outcome in outcomes:
        for finding in outcome.findings:
            finding.file_path = relativise(finding.file_path, root)
        findings.extend(outcome.findings)
        if outcome.status == "unavailable":
            # run_tool already logged the detail; no second copy here
            unavailable.append(outcome.tool)
        elif outcome.status == "skipped":
            logger.debug(f"{outcome.tool}: {outcome.detail}")

    return findings, unavailable
