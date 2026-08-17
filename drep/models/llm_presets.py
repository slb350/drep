"""Named LLM providers for the setup wizard and the installer.

Every cloud option here speaks the OpenAI-compatible protocol, so the
difference between them is an endpoint, a model, and which environment
variable holds the key. A preset is what stops a user having to know that -
they pick "OpenRouter", not "openai-compatible with base URL
https://openrouter.ai/api/v1".

Model defaults are the one thing here that goes stale. They are starting
points the wizard offers, always editable at the prompt, and never silently
imposed.
"""

from dataclasses import dataclass

# Reasoning models bill their `reasoning` trace against the completion budget
# and return empty content if they exhaust it, which surfaces as "LLM returned
# no content (finish_reason='length')". Cloud presets default high enough to
# clear a full trace plus the answer, with a wall clock to match.
_REASONING_MAX_TOKENS = 100_000
_REASONING_TIMEOUT = 1_800


@dataclass(frozen=True)
class LLMPreset:
    """One named way to reach a model.

    Attributes:
        display_name: What the wizard shows.
        description: One line on when to pick it.
        endpoint: Base URL, or None if the user must supply one.
        default_model: Starting point for the model prompt, or None.
        api_key_env: Environment variable holding the key, or None if the
            provider needs no credentials. The variable *name* goes into
            config.yaml as a `${...}` placeholder - never the key itself,
            because config.yaml is usually committed.
        max_tokens: Completion budget.
        timeout: Request timeout in seconds.
    """

    display_name: str
    description: str
    endpoint: str | None
    default_model: str | None
    api_key_env: str | None
    max_tokens: int = 8_000
    timeout: int = 120

    def to_config(self, model: str, endpoint: str | None = None) -> dict:
        """Render the `llm` block of config.yaml for this preset."""
        config: dict = {
            "enabled": True,
            "provider": "openai-compatible",
            "endpoint": endpoint or self.endpoint,
            "model": model,
            "max_tokens": self.max_tokens,
            "timeout": self.timeout,
        }
        if self.api_key_env:
            config["api_key"] = f"${{{self.api_key_env}}}"
        return config


LLM_PRESETS: dict[str, LLMPreset] = {
    "local": LLMPreset(
        display_name="Local model",
        description="LM Studio, Ollama or llama.cpp on this machine. No key, no cost.",
        endpoint="http://localhost:1234/v1",
        default_model="qwen3-30b-a3b",
        api_key_env=None,
    ),
    "openrouter": LLMPreset(
        display_name="OpenRouter",
        description="One key for many providers. Good default for cloud analysis.",
        endpoint="https://openrouter.ai/api/v1",
        default_model="deepseek/deepseek-v4-pro-0813",
        api_key_env="OPENROUTER_API_KEY",
        max_tokens=_REASONING_MAX_TOKENS,
        timeout=_REASONING_TIMEOUT,
    ),
    "openai": LLMPreset(
        display_name="OpenAI",
        description="Directly against the OpenAI API.",
        endpoint="https://api.openai.com/v1",
        default_model="gpt-5.6-sol",
        api_key_env="OPENAI_API_KEY",
        max_tokens=_REASONING_MAX_TOKENS,
        timeout=_REASONING_TIMEOUT,
    ),
    "custom": LLMPreset(
        display_name="Custom endpoint",
        description="Any other OpenAI-compatible endpoint.",
        endpoint=None,
        default_model=None,
        api_key_env="LLM_API_KEY",
    ),
}


def preset_names() -> list[str]:
    """Preset keys, in the order the wizard should offer them."""
    return list(LLM_PRESETS)
