"""Tests for custom Click parameter validators."""

import pytest
from click import BadParameter

from drep.cli_validators import (
    BedrockModelType,
    DatabaseURLType,
    NonEmptyString,
    RepositoryListType,
    URLType,
)


class TestURLType:
    """Tests for URLType validator."""

    def test_valid_http_url(self):
        """Test valid HTTP URL is accepted."""
        validator = URLType()
        result = validator.convert("http://localhost:3000", None, None)
        assert result == "http://localhost:3000"

    def test_valid_https_url(self):
        """Test valid HTTPS URL is accepted."""
        validator = URLType()
        result = validator.convert("https://github.com/api/v3", None, None)
        assert result == "https://github.com/api/v3"

    def test_url_with_port(self):
        """Test URL with port number is accepted."""
        validator = URLType()
        result = validator.convert("https://gitlab.com:8080", None, None)
        assert result == "https://gitlab.com:8080"

    def test_url_with_path(self):
        """Test URL with path is accepted."""
        validator = URLType()
        result = validator.convert("http://example.com/api/v1", None, None)
        assert result == "http://example.com/api/v1"

    def test_missing_protocol(self):
        """Test URL without protocol is rejected."""
        validator = URLType()
        with pytest.raises(BadParameter, match="missing URL scheme"):
            validator.convert("localhost:3000", None, None)

    def test_invalid_protocol(self):
        """Test URL with non-HTTP protocol is rejected."""
        validator = URLType()
        with pytest.raises(BadParameter, match="missing URL scheme"):
            validator.convert("ftp://example.com", None, None)

    def test_malformed_url(self):
        """Test malformed URL is rejected."""
        validator = URLType()
        with pytest.raises(BadParameter, match="missing hostname"):
            validator.convert("http://", None, None)

    def test_empty_url(self):
        """Test empty URL is rejected."""
        validator = URLType()
        with pytest.raises(BadParameter, match="cannot be empty"):
            validator.convert("", None, None)


class TestRepositoryListType:
    """Tests for RepositoryListType validator."""

    def test_single_repository(self):
        """Test single repository pattern is accepted."""
        validator = RepositoryListType()
        result = validator.convert("owner/repo", None, None)
        assert result == ["owner/repo"]

    def test_multiple_repositories(self):
        """Test comma-separated repository patterns are accepted."""
        validator = RepositoryListType()
        result = validator.convert("owner/repo1, owner/repo2", None, None)
        assert result == ["owner/repo1", "owner/repo2"]

    def test_wildcard_pattern(self):
        """Test wildcard pattern is accepted."""
        validator = RepositoryListType()
        result = validator.convert("owner/*", None, None)
        assert result == ["owner/*"]

    def test_filters_empty_strings(self):
        """Test empty strings from extra commas are filtered."""
        validator = RepositoryListType()
        result = validator.convert("owner/repo1, , owner/repo2", None, None)
        assert result == ["owner/repo1", "owner/repo2"]

    def test_trailing_comma(self):
        """Test trailing comma is handled correctly."""
        validator = RepositoryListType()
        result = validator.convert("owner/repo,", None, None)
        assert result == ["owner/repo"]

    def test_missing_owner(self):
        """Test repository without owner is rejected."""
        validator = RepositoryListType()
        with pytest.raises(BadParameter, match="Invalid repository pattern"):
            validator.convert("repo", None, None)

    def test_too_many_slashes(self):
        """Test repository with multiple slashes is rejected."""
        validator = RepositoryListType()
        with pytest.raises(BadParameter, match="Invalid repository pattern"):
            validator.convert("owner/repo/extra", None, None)

    def test_empty_input(self):
        """Test empty input is rejected."""
        validator = RepositoryListType()
        with pytest.raises(BadParameter, match="Must provide at least one"):
            validator.convert("", None, None)

    def test_only_commas(self):
        """Test input with only commas is rejected."""
        validator = RepositoryListType()
        with pytest.raises(BadParameter, match="Must provide at least one"):
            validator.convert(",,,", None, None)


class TestBedrockModelType:
    """Tests for BedrockModelType validator."""

    def test_anthropic_model(self):
        """Test Anthropic model ID is accepted."""
        validator = BedrockModelType()
        result = validator.convert("anthropic.claude-sonnet-4-5-20250929-v1:0", None, None)
        assert result == "anthropic.claude-sonnet-4-5-20250929-v1:0"

    def test_global_anthropic_model(self):
        """Test global Anthropic model ID is accepted."""
        validator = BedrockModelType()
        result = validator.convert("global.anthropic.claude-v2", None, None)
        assert result == "global.anthropic.claude-v2"

    def test_amazon_model(self):
        """Test Amazon model ID is accepted."""
        validator = BedrockModelType()
        result = validator.convert("amazon.titan-text-express-v1", None, None)
        assert result == "amazon.titan-text-express-v1"

    def test_meta_model(self):
        """Test Meta model ID is accepted."""
        validator = BedrockModelType()
        result = validator.convert("meta.llama2-13b-chat-v1", None, None)
        assert result == "meta.llama2-13b-chat-v1"

    def test_cohere_model(self):
        """Test Cohere model ID is accepted."""
        validator = BedrockModelType()
        result = validator.convert("cohere.command-text-v14", None, None)
        assert result == "cohere.command-text-v14"

    def test_invalid_prefix(self):
        """Test model ID with invalid prefix is rejected."""
        validator = BedrockModelType()
        with pytest.raises(BadParameter, match="Invalid Bedrock model ID"):
            validator.convert("openai.gpt-4", None, None)

    def test_empty_model_id(self):
        """Test empty model ID is rejected."""
        validator = BedrockModelType()
        with pytest.raises(BadParameter, match="cannot be empty"):
            validator.convert("", None, None)


class TestDatabaseURLType:
    """Tests for DatabaseURLType validator."""

    def test_sqlite_url(self):
        """Test SQLite database URL is accepted."""
        validator = DatabaseURLType()
        result = validator.convert("sqlite:///./drep.db", None, None)
        assert result == "sqlite:///./drep.db"

    def test_postgresql_url(self):
        """Test PostgreSQL database URL is accepted."""
        validator = DatabaseURLType()
        result = validator.convert("postgresql://user:pass@localhost/dbname", None, None)
        assert result == "postgresql://user:pass@localhost/dbname"

    def test_mysql_url(self):
        """Test MySQL database URL is accepted."""
        validator = DatabaseURLType()
        result = validator.convert("mysql://user:pass@localhost/dbname", None, None)
        assert result == "mysql://user:pass@localhost/dbname"

    def test_missing_separator(self):
        """Test URL without :// is rejected."""
        validator = DatabaseURLType()
        with pytest.raises(BadParameter, match="Must contain '://'"):
            validator.convert("sqlite:drep.db", None, None)

    def test_empty_url(self):
        """Test empty URL is rejected."""
        validator = DatabaseURLType()
        with pytest.raises(BadParameter, match="cannot be empty"):
            validator.convert("", None, None)


class TestNonEmptyString:
    """Tests for NonEmptyString validator."""

    def test_valid_string(self):
        """Test non-empty string is accepted."""
        validator = NonEmptyString()
        result = validator.convert("qwen3-30b-a3b", None, None)
        assert result == "qwen3-30b-a3b"

    def test_string_with_whitespace(self):
        """Test string with leading/trailing whitespace is stripped."""
        validator = NonEmptyString()
        result = validator.convert("  model-name  ", None, None)
        assert result == "model-name"

    def test_empty_string(self):
        """Test empty string is rejected."""
        validator = NonEmptyString()
        with pytest.raises(BadParameter, match="cannot be empty"):
            validator.convert("", None, None)

    def test_whitespace_only(self):
        """Test whitespace-only string is rejected."""
        validator = NonEmptyString()
        with pytest.raises(BadParameter, match="cannot be empty"):
            validator.convert("   ", None, None)
