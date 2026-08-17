"""LLM provider preset tests.

A preset is the difference between "enter an OpenAI-compatible endpoint URL"
and "choose OpenRouter". Every cloud provider here is openai-compatible under
the hood; the preset is what stops the user having to know that.
"""

import pytest

from drep.models.llm_presets import LLM_PRESETS, preset_names


class TestPresetCoverage:
    def test_the_three_the_installer_offers_exist(self):
        assert {"local", "openai", "openrouter"} <= set(preset_names())

    def test_custom_is_offered_for_anything_else(self):
        assert "custom" in preset_names()

    def test_every_preset_names_itself(self):
        for name, preset in LLM_PRESETS.items():
            assert preset.display_name, f"{name} has no display name"
            assert preset.description, f"{name} has no description"


class TestEndpointsAndKeys:
    """A preset carries the endpoint and the env var its key comes from."""

    def test_local_needs_no_api_key(self):
        """A local model is the reason drep works with no credentials at all."""
        local = LLM_PRESETS["local"]
        assert local.api_key_env is None
        assert "localhost" in str(local.endpoint)

    @pytest.mark.parametrize(
        ("name", "host"), [("openai", "api.openai.com"), ("openrouter", "openrouter.ai")]
    )
    def test_cloud_presets_point_at_their_provider(self, name, host):
        assert host in str(LLM_PRESETS[name].endpoint)

    def test_cloud_presets_take_their_key_from_the_environment(self):
        """Never written into config.yaml, which is committed in most repos."""
        for name in ("openai", "openrouter"):
            preset = LLM_PRESETS[name]
            assert preset.api_key_env
            assert preset.api_key_env.isupper()

    def test_custom_presumes_nothing(self):
        custom = LLM_PRESETS["custom"]
        assert custom.endpoint is None
        assert custom.default_model is None


class TestConfigGeneration:
    """A preset renders into the llm block of config.yaml."""

    def test_local_renders_without_an_api_key_field(self):
        config = LLM_PRESETS["local"].to_config(model="qwen3-30b-a3b")
        assert config["enabled"] is True
        assert config["provider"] == "openai-compatible"
        assert "api_key" not in config

    def test_cloud_renders_an_env_placeholder_not_the_secret(self):
        config = LLM_PRESETS["openrouter"].to_config(model="x/y")
        assert config["api_key"] == "${OPENROUTER_API_KEY}"

    def test_reasoning_models_get_a_budget_that_fits_them(self):
        """A reasoning model that exhausts max_tokens on `reasoning` returns no
        content at all, so the cloud presets ship a budget that clears one."""
        config = LLM_PRESETS["openrouter"].to_config(model="deepseek/deepseek-v4-pro-0813")
        assert config["max_tokens"] >= 100000
        assert config["timeout"] >= 600

    def test_generated_config_validates(self):
        from drep.models.config import LLMConfig

        for name in ("local", "openai", "openrouter"):
            preset = LLM_PRESETS[name]
            config = preset.to_config(model=preset.default_model or "some-model")
            # api_key placeholders are substituted at load time, not here
            config.pop("api_key", None)
            LLMConfig(**config)
