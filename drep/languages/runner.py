"""Run a project's own deterministic checkers and turn their output into Findings.

This is the gating half of analysis. Tool findings are precise - they come from
the rules the project itself configured - so they block, while the LLM's
semantic findings inform. Keeping the two apart by *source* rather than by
severity is what makes the gate calibratable at all.

Three states, deliberately distinct:

- ``ok``          the tool ran; its findings are authoritative
- ``skipped``     the project has not configured this tool, so it has no
                  opinion here. A pass.
- ``unavailable`` the tool should have run and could not. **Not** a pass -
                  reporting it as clean is the same "unanalyzed is not clean"
                  mistake that made `drep check` rubber-stamp commits.
"""

import asyncio
import json
import logging
import re
import shutil
from dataclasses import dataclass, field
from pathlib import Path

from drep.languages.base import ToolSpec
from drep.models.findings import Finding

logger = logging.getLogger(__name__)

# A tool that has not produced output by now is hung, not slow. Deterministic
# checkers are fast; this is not the LLM path.
TOOL_TIMEOUT_SECONDS = 120

# `[vet: ]./path/to/file.go:12:6: message` - the compiler-style position that
# go vet and most Go tooling emit.
_POSITION = re.compile(
    r"^(?:vet:\s*)?(?P<file>[^\s:][^:]*):(?P<line>\d+):(?P<col>\d+):\s*(?P<message>.+)$"
)

# `src/app.ts(14,22): error TS2345: message` - the tsc shape.
_TSC = re.compile(
    r"^(?P<file>.+?)\((?P<line>\d+),(?P<col>\d+)\):\s*"
    r"(?:error|warning)\s+(?P<code>TS\d+):\s*(?P<message>.+)$"
)


class ToolOutputError(RuntimeError):
    """A tool produced output we could not parse.

    Raised rather than swallowed: unparseable output means we do not know
    whether the file is clean, and guessing "clean" is the failure this module
    exists to prevent.
    """


@dataclass
class ToolOutcome:
    """What happened when one tool was asked to check some files."""

    tool: str
    status: str  # "ok" | "skipped" | "unavailable"
    findings: list[Finding] = field(default_factory=list)
    detail: str = ""

    @property
    def passed(self) -> bool:
        """Whether this outcome is safe to treat as "nothing wrong here".

        `unavailable` is not: the check never happened.
        """
        return self.status != "unavailable"


def resolve_tool(spec: ToolSpec, root: Path) -> Path | None:
    """Find the executable for a tool, preferring the project's own copy.

    Repo-local first so a project is checked by the version its CI runs -
    `node_modules/.bin/eslint` rather than whatever happens to be installed
    globally, which may resolve plugins differently or not at all.

    Returns:
        Path to the executable, or None if it cannot be found.
    """
    for relative in spec.local_paths:
        candidate = root / relative
        if candidate.is_file() and candidate.stat().st_mode & 0o111:
            return candidate

    found = shutil.which(spec.command[0])
    return Path(found) if found else None


def is_configured(spec: ToolSpec, root: Path) -> bool:
    """Whether the project has opted into this tool.

    "Style adherence where defined": a repo with no eslint config has not
    chosen eslint's defaults, so running it would invent findings the project
    never asked for.
    """
    return any((root / name).exists() for name in spec.config_files)


def parse_output(spec: ToolSpec, stdout: str, root_name: str) -> list[Finding]:
    """Convert a tool's stdout into Findings.

    Every tool's own shape is normalised here, so nothing downstream of this
    module knows which tool produced a finding.

    Raises:
        ToolOutputError: The output could not be parsed.
    """
    if spec.output_format == "lines":
        # gofmt -l prints one path per line: the file needs formatting.
        return [
            Finding(
                type=spec.name,
                severity="error",
                file_path=line.strip(),
                line=1,
                column=None,
                message=f"{spec.name}: file is not formatted",
                suggestion=f"Run `{' '.join(spec.command[:-1])} -w {line.strip()}`",
            )
            for line in stdout.splitlines()
            if line.strip()
        ]

    if spec.output_format == "json":
        try:
            payload = json.loads(stdout or "[]")
        except json.JSONDecodeError as exc:
            raise ToolOutputError(f"{spec.name} produced unparseable JSON: {exc}") from exc
        return _parse_json_payload(spec, payload, root_name)

    if spec.output_format == "position":
        return _parse_positions(spec, stdout)

    if spec.output_format == "tsc":
        return _parse_tsc(spec, stdout)

    if spec.output_format == "cargo":
        return _parse_cargo(spec, stdout)

    raise ToolOutputError(f"{spec.name}: no parser for output format {spec.output_format!r}")


def _parse_positions(spec: ToolSpec, output: str) -> list[Finding]:
    """`file:line:col: message`, skipping the package headers Go interleaves."""
    findings = []
    for line in output.splitlines():
        match = _POSITION.match(line.strip())
        if not match:
            # `# example.com/pkg` headers and blank lines are not diagnostics
            continue
        findings.append(
            Finding(
                type=spec.name,
                severity="error",
                file_path=match.group("file").removeprefix("./"),
                line=int(match.group("line")),
                column=int(match.group("col")),
                message=match.group("message").strip(),
            )
        )
    return findings


def _parse_tsc(spec: ToolSpec, output: str) -> list[Finding]:
    """`file(line,col): error TS1234: message`."""
    findings = []
    for line in output.splitlines():
        match = _TSC.match(line.strip())
        if not match:
            continue
        findings.append(
            Finding(
                type=match.group("code"),
                severity="error",
                file_path=match.group("file"),
                line=int(match.group("line")),
                column=int(match.group("col")),
                message=match.group("message").strip(),
            )
        )
    return findings


def _parse_cargo(spec: ToolSpec, output: str) -> list[Finding]:
    """cargo's newline-delimited JSON: one object per event, not one array.

    Only `compiler-message` events are diagnostics; the rest are build
    progress. A line that is not JSON at all is an error rather than a skip,
    since that means we are not reading what we think we are.
    """
    findings = []
    for line in output.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as exc:
            raise ToolOutputError(f"{spec.name} emitted a non-JSON line: {exc}") from exc

        if not isinstance(event, dict) or event.get("reason") != "compiler-message":
            continue

        message = event.get("message") or {}
        spans = [s for s in message.get("spans", []) if s.get("is_primary")]
        span = spans[0] if spans else {}
        findings.append(
            Finding(
                type=(message.get("code") or {}).get("code") or spec.name,
                severity="error",
                file_path=span.get("file_name", ""),
                line=span.get("line_start") or 1,
                column=span.get("column_start"),
                message=message.get("message", ""),
            )
        )
    return findings


def _parse_json_payload(spec: ToolSpec, payload: object, root_name: str) -> list[Finding]:
    """Normalise ruff/eslint-shaped JSON into Findings."""
    findings: list[Finding] = []

    if not isinstance(payload, list):
        raise ToolOutputError(f"{spec.name}: expected a JSON array, got {type(payload).__name__}")

    for entry in payload:
        if not isinstance(entry, dict):
            raise ToolOutputError(f"{spec.name}: expected objects in the array")

        # Flat records with a `location` (ruff's shape)
        if "location" in entry:
            location = entry.get("location") or {}
            findings.append(
                Finding(
                    type=entry.get("code") or spec.name,
                    severity="error",
                    file_path=entry.get("filename") or root_name,
                    line=location.get("row") or 1,
                    column=location.get("column"),
                    message=entry.get("message", ""),
                    suggestion=(entry.get("fix") or {}).get("message"),
                )
            )
            continue

        # eslint: one record per file, with a nested messages array
        findings.extend(
            Finding(
                type=message.get("ruleId") or spec.name,
                severity="error",
                file_path=entry.get("filePath") or root_name,
                line=message.get("line") or 1,
                column=message.get("column"),
                message=message.get("message", ""),
                suggestion=None,
            )
            for message in entry.get("messages", [])
        )

    return findings


async def run_tool(
    spec: ToolSpec,
    root: Path,
    files: list[str],
    *,
    _force_missing: bool = False,
) -> ToolOutcome:
    """Run one deterministic tool over some files.

    Args:
        spec: The tool to run.
        root: Repository root; tool resolution and config detection are
            relative to it, and it is the working directory for the run.
        files: Repo-relative paths to check.
        _force_missing: Test hook to simulate an absent binary.

    Returns:
        A ToolOutcome. Never raises for an absent or failing tool - that is
        reported as `unavailable` so the caller can surface it rather than
        mistake it for a clean result.
    """
    if not is_configured(spec, root):
        return ToolOutcome(
            tool=spec.name,
            status="skipped",
            detail=f"{spec.name} is not configured in this project",
        )

    executable = None if _force_missing else resolve_tool(spec, root)
    if executable is None:
        detail = (
            f"{spec.name} is configured but was not found "
            f"(looked in {', '.join(spec.local_paths) or 'PATH'} then PATH)"
        )
        logger.warning(detail)
        return ToolOutcome(tool=spec.name, status="unavailable", detail=detail)

    argv = [str(executable), *spec.command[1:], *files]
    try:
        process = await asyncio.create_subprocess_exec(
            *argv,
            cwd=root,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        raw_stdout, raw_stderr = await asyncio.wait_for(
            process.communicate(), timeout=TOOL_TIMEOUT_SECONDS
        )
    except TimeoutError:
        detail = f"{spec.name} timed out after {TOOL_TIMEOUT_SECONDS}s"
        logger.error(detail)
        return ToolOutcome(tool=spec.name, status="unavailable", detail=detail)
    except OSError as exc:
        detail = f"{spec.name} could not be executed: {exc}"
        logger.error(detail)
        return ToolOutcome(tool=spec.name, status="unavailable", detail=detail)

    stdout = raw_stdout.decode("utf-8", errors="replace")
    stderr = raw_stderr.decode("utf-8", errors="replace")
    diagnostics = stderr if spec.diagnostics_stream == "stderr" else stdout
    other = stdout if spec.diagnostics_stream == "stderr" else stderr

    try:
        findings = parse_output(spec, diagnostics, root_name=files[0] if files else "")
    except ToolOutputError as exc:
        # Non-zero exit with unparseable output usually means the tool itself
        # failed (bad config, missing plugin) rather than that it found issues.
        detail = f"{exc}. other stream: {other.strip()[:200]}"
        logger.error(detail)
        return ToolOutcome(tool=spec.name, status="unavailable", detail=detail)

    return ToolOutcome(tool=spec.name, status="ok", findings=findings, detail=other.strip()[:200])
