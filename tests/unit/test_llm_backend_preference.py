"""Tests ensuring LLMClient prefers open-agent-sdk when available."""

from unittest.mock import AsyncMock, MagicMock

import pytest

from drep.llm.client import LLMClient


@pytest.mark.asyncio
async def test_llmclient_uses_open_agent_sdk(monkeypatch):
    """LLMClient should prefer open-agent-sdk client if import succeeds."""

    # Build a dummy client shape that mimics AsyncOpenAI
    class DummyMsg:
        def __init__(self, content: str):
            self.content = content

    class DummyChoice:
        def __init__(self, content: str):
            self.message = DummyMsg(content)

    class DummyUsage:
        def __init__(self):
            self.prompt_tokens = 10
            self.completion_tokens = 20
            self.total_tokens = 30

    class DummyResponse:
        def __init__(self, content: str):
            self.model = "dummy-model"
            self.choices = [DummyChoice(content)]
            self.usage = DummyUsage()

    class DummyCompletions:
        async def create(self, **kwargs):  # noqa: ARG002
            return DummyResponse("hello from open-agent-sdk")

    class DummyChat:
        def __init__(self):
            self.completions = DummyCompletions()

    class DummyClient:
        def __init__(self, *args, **kwargs):  # noqa: ARG002
            self.chat = DummyChat()

        async def close(self):
            return None

    # Patch open_agent.create_client to return our dummy
    monkeypatch.setattr("open_agent.utils.create_client", lambda options: DummyClient())

    client = LLMClient(endpoint="http://localhost:1234/v1", model="test-model")

    # Call analyze_code and verify it uses our dummy
    result = await client.analyze_code(system_prompt="Say hi", code="")
    assert result.content == "hello from open-agent-sdk"
    assert result.tokens_used == 30
    await client.close()
