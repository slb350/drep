"""OpenAI-shaped shim over the raw HTTP backend.

Wraps an httpx response so both backends expose the same surface
(``client.chat.completions.create`` -> ``response.choices[0].message.content``),
which also lets tests mock one interface regardless of which backend is active.

Split out of ``drep/llm/client.py`` for file size. These were previously nested
inside ``LLMClient.__init__``, so they were rebuilt on every client construction
and each instance closed over the whole ``__init__`` frame — keeping api_key,
headers and options reachable for the client's lifetime. They now take the
parent client explicitly.
"""

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from drep.llm.client import LLMClient


class _CompatMessage:
    def __init__(self, content: str):
        self.content = content


class _CompatChoice:
    def __init__(self, content: str):
        self.message = _CompatMessage(content)


class _CompatUsage:
    def __init__(self, usage: dict[str, Any]):
        prompt = usage.get("prompt_tokens") or usage.get("input_tokens") or 0
        completion = usage.get("completion_tokens") or usage.get("output_tokens") or 0
        self.prompt_tokens = prompt
        self.completion_tokens = completion
        self.total_tokens = usage.get("total_tokens") or (prompt + completion)


class _CompatResponse:
    def __init__(self, data: dict[str, Any], default_model: str):
        self.model = data.get("model", default_model)
        content = (
            ((data.get("choices") or [{}])[0].get("message") or {}).get("content")
            or data.get("content")
            or ""
        )
        self.choices = [_CompatChoice(content)]
        self.usage = _CompatUsage(data.get("usage", {}))


class _CompatCompletions:
    def __init__(self, parent: "LLMClient"):
        self._parent = parent

    async def create(self, model: str, messages: list, temperature: float, max_tokens: int):
        if not self._parent.http:
            raise RuntimeError("HTTP client not initialized")
        url = f"{self._parent.endpoint}/chat/completions"
        payload = {
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        }
        resp = await self._parent.http.post(url, json=payload)
        resp.raise_for_status()
        return _CompatResponse(resp.json(), self._parent.model)


class _CompatChat:
    def __init__(self, parent: "LLMClient"):
        self.completions = _CompatCompletions(parent)


class _CompatClient:
    def __init__(self, parent: "LLMClient"):
        self._parent = parent
        self.chat = _CompatChat(parent)

    async def close(self):
        if self._parent.http:
            await self._parent.http.aclose()
