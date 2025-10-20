"""Tests ensuring the tightened docstring prompt is used."""

from unittest.mock import AsyncMock, MagicMock

import pytest

from drep.docstring.generator import DocstringGenerator
from drep.docstring.ast_utils import FunctionInfo


@pytest.mark.asyncio
async def test_docstring_generator_uses_v2_prompt(monkeypatch):
    """DocstringGenerator should use the stricter JSON-only prompt (V2)."""
    captured = {"system_prompt": None}

    async def capture_analyze_code_json(*args, **kwargs):  # noqa: ARG001
        captured["system_prompt"] = kwargs.get("system_prompt")
        return {"docstring": "A.\n\nReturns:\n    None", "quality": "high", "reasoning": "Test"}

    mock_llm = MagicMock()
    mock_llm.analyze_code_json = AsyncMock(side_effect=capture_analyze_code_json)
    gen = DocstringGenerator(mock_llm)

    func = FunctionInfo(
        name="foo",
        line_number=10,
        docstring=None,
        args=["x"],
        returns=None,
        is_public=True,
        complexity=5,
        decorators=[],
    )

    code = "def foo(x):\n    return x\n"

    await gen._generate_docstring(
        file_path="mod.py",
        func_info=func,
        full_content=code,
        repo_id="owner/repo",
        commit_sha="deadbeef",
    )

    assert captured["system_prompt"] is not None
    assert "Output JSON only with exactly these keys" in captured["system_prompt"]
