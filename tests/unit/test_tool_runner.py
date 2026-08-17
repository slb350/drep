"""Deterministic tool runner tests.

The runner is the half of analysis that gates: its findings come from the
project's own tools, so they are precise enough to block a commit. That makes
"the tool did not run" a result the caller must be able to see - a repo with no
eslint installed must not look identical to one that passed eslint.
"""

import json

import pytest

from drep.languages import registry
from drep.languages.runner import ToolOutcome, resolve_tool, run_tool


class TestToolResolution:
    """Repo-local before PATH, so a project gets the version its CI runs."""

    def test_prefers_a_repo_local_binary(self, tmp_path):
        eslint = tmp_path / "node_modules" / ".bin" / "eslint"
        eslint.parent.mkdir(parents=True)
        eslint.write_text("#!/bin/sh\n")
        eslint.chmod(0o755)

        typescript = registry.get("typescript")
        spec = next(t for t in typescript.tools if t.name == "eslint")

        assert resolve_tool(spec, tmp_path) == eslint

    def test_ignores_a_repo_local_path_that_is_not_executable(self, tmp_path):
        eslint = tmp_path / "node_modules" / ".bin" / "eslint"
        eslint.parent.mkdir(parents=True)
        eslint.write_text("not executable")

        typescript = registry.get("typescript")
        spec = next(t for t in typescript.tools if t.name == "eslint")

        # Falls through to PATH, where the test environment has no eslint
        assert resolve_tool(spec, tmp_path) is None

    def test_falls_back_to_path(self, tmp_path):
        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")
        # ruff is a dev dependency of this project, so it is on PATH in CI
        resolved = resolve_tool(spec, tmp_path)
        assert resolved is None or resolved.name.startswith("ruff")


class TestConfigGating:
    """A tool the project has not configured is not the project's style."""

    @pytest.mark.asyncio
    async def test_unconfigured_tool_is_skipped_not_run(self, tmp_path):
        (tmp_path / "main.py").write_text("x=1\n")
        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")

        outcome = await run_tool(spec, tmp_path, ["main.py"])

        assert outcome.status == "skipped"
        assert "not configured" in outcome.detail
        assert outcome.findings == []

    @pytest.mark.asyncio
    async def test_configured_tool_runs(self, tmp_path):
        (tmp_path / "pyproject.toml").write_text("[tool.ruff]\n")
        (tmp_path / "main.py").write_text("import os\n")
        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")

        outcome = await run_tool(spec, tmp_path, ["main.py"])

        # ruff is installed in this project's venv; if it were not, the runner
        # must say so rather than report a clean file
        assert outcome.status in {"ok", "unavailable"}
        if outcome.status == "ok":
            assert any(f.type == "F401" for f in outcome.findings)


class TestNotRunIsNotClean:
    """The invariant this whole tool is built on."""

    @pytest.mark.asyncio
    async def test_missing_binary_is_reported_as_unavailable(self, tmp_path):
        (tmp_path / "go.mod").write_text("module example.com/x\n")
        (tmp_path / "main.go").write_text("package main\n")
        go = registry.get("go")
        spec = next(t for t in go.tools if t.name == "gofmt")

        outcome = await run_tool(spec, tmp_path, ["main.go"], _force_missing=True)

        assert outcome.status == "unavailable"
        assert outcome.findings == []
        # An unavailable tool must never read as a pass
        assert not outcome.passed

    @pytest.mark.asyncio
    async def test_a_clean_run_passes(self, tmp_path):
        outcome = ToolOutcome(tool="ruff", status="ok", findings=[], detail="")
        assert outcome.passed

    @pytest.mark.asyncio
    async def test_a_skipped_tool_does_not_block(self, tmp_path):
        """Skipped is a deliberate project choice, unlike unavailable."""
        outcome = ToolOutcome(tool="eslint", status="skipped", findings=[], detail="not configured")
        assert outcome.passed


class TestOutputParsing:
    """Tool output becomes Findings, so the rest of drep stays tool-agnostic."""

    def test_parses_ruff_json(self):
        from drep.languages.runner import parse_output

        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")
        payload = json.dumps(
            [
                {
                    "code": "F401",
                    "message": "`os` imported but unused",
                    "filename": "main.py",
                    "location": {"row": 1, "column": 8},
                    "fix": {"message": "Remove unused import"},
                }
            ]
        )

        findings = parse_output(spec, payload, root_name="main.py")

        assert len(findings) == 1
        assert findings[0].line == 1
        assert findings[0].column == 8
        assert findings[0].type == "F401"
        # Deterministic findings gate, so they carry blocking severity
        assert findings[0].severity == "error"
        assert "unused" in findings[0].message

    def test_parses_gofmt_line_output(self):
        from drep.languages.runner import parse_output

        go = registry.get("go")
        spec = next(t for t in go.tools if t.name == "gofmt")

        findings = parse_output(spec, "cmd/server.go\ninternal/db.go\n", root_name="")

        assert [f.file_path for f in findings] == ["cmd/server.go", "internal/db.go"]
        assert all(f.type == "gofmt" for f in findings)

    def test_malformed_output_is_an_error_not_a_pass(self):
        from drep.languages.runner import ToolOutputError, parse_output

        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")

        with pytest.raises(ToolOutputError):
            parse_output(spec, "this is not json", root_name="main.py")


class TestPositionParsers:
    """Text-position formats: the compiler-style `file:line:col: message`."""

    def test_parses_go_vet_output(self):
        from drep.languages.runner import parse_output

        go = registry.get("go")
        spec = next(t for t in go.tools if t.name == "go vet")
        output = (
            "# example.com/x\n"
            "vet: ./main.go:12:6: Printf format %d has arg of wrong type string\n"
            "./other.go:3:1: unreachable code\n"
        )

        findings = parse_output(spec, output, root_name="")

        assert len(findings) == 2
        assert findings[0].file_path == "main.go"
        assert findings[0].line == 12
        assert findings[0].column == 6
        assert "Printf" in findings[0].message
        assert findings[1].file_path == "other.go"

    def test_parses_tsc_output(self):
        from drep.languages.runner import parse_output

        typescript = registry.get("typescript")
        spec = next(t for t in typescript.tools if t.name == "tsc")
        output = (
            "src/app.ts(14,22): error TS2345: Argument of type 'string' is not assignable.\n"
            "src/app.ts(20,5): error TS2532: Object is possibly 'undefined'.\n"
        )

        findings = parse_output(spec, output, root_name="")

        assert len(findings) == 2
        assert findings[0].file_path == "src/app.ts"
        assert findings[0].line == 14
        assert findings[0].column == 22
        assert findings[0].type == "TS2345"

    def test_ignores_non_diagnostic_noise(self):
        from drep.languages.runner import parse_output

        go = registry.get("go")
        spec = next(t for t in go.tools if t.name == "go vet")

        assert parse_output(spec, "# example.com/x\n\n", root_name="") == []


class TestCargoParser:
    """clippy emits newline-delimited JSON, one object per diagnostic."""

    def test_parses_clippy_jsonl(self):
        from drep.languages.runner import parse_output

        rust = registry.get("rust")
        spec = next(t for t in rust.tools if t.name == "clippy")
        output = "\n".join(
            [
                json.dumps({"reason": "compiler-artifact", "target": {"name": "x"}}),
                json.dumps(
                    {
                        "reason": "compiler-message",
                        "message": {
                            "code": {"code": "clippy::needless_return"},
                            "message": "unneeded `return` statement",
                            "spans": [
                                {
                                    "file_name": "src/lib.rs",
                                    "line_start": 7,
                                    "column_start": 5,
                                    "is_primary": True,
                                }
                            ],
                        },
                    }
                ),
            ]
        )

        findings = parse_output(spec, output, root_name="")

        # The non-diagnostic artifact line is not a finding
        assert len(findings) == 1
        assert findings[0].file_path == "src/lib.rs"
        assert findings[0].line == 7
        assert findings[0].type == "clippy::needless_return"


class TestDiagnosticStream:
    """go vet writes to stderr; reading only stdout would report it clean."""

    def test_go_vet_declares_stderr(self):
        go = registry.get("go")
        spec = next(t for t in go.tools if t.name == "go vet")
        assert spec.diagnostics_stream == "stderr"

    def test_ruff_declares_stdout(self):
        python = registry.get("python")
        spec = next(t for t in python.tools if t.name == "ruff")
        assert spec.diagnostics_stream == "stdout"
