"""Tests for drep.security module - secret detection and logging safety."""


def test_security_module_exists():
    """Test that security module exists."""
    from drep import security

    assert security is not None


def test_detect_secrets_in_logs_basic_patterns():
    """Test that detect_secrets_in_logs identifies common secret patterns."""
    from drep.security.detector import detect_secrets_in_logs

    # Should detect token patterns
    assert detect_secrets_in_logs("Authorization: Bearer abc123token") is True
    assert detect_secrets_in_logs("token=secret_value_here") is True
    assert detect_secrets_in_logs("api_key=sk-1234567890") is True
    assert detect_secrets_in_logs("password=MyP@ssw0rd") is True
    assert detect_secrets_in_logs("secret=confidential") is True

    # Should detect OAuth tokens (CRITICAL for OAuth flows)
    assert detect_secrets_in_logs("access_token=ya29.abc123") is True
    assert detect_secrets_in_logs("oauth_token=oauth_xyz") is True
    assert detect_secrets_in_logs("refresh_token=1//abc") is True
    assert detect_secrets_in_logs("client_secret=secret123") is True
    assert detect_secrets_in_logs("client_id=12345") is True

    # Should detect standalone Bearer tokens
    assert detect_secrets_in_logs("Bearer abc123") is True
    assert detect_secrets_in_logs("Using Bearer xyz789") is True

    # Should detect private keys
    assert detect_secrets_in_logs("private_key=-----BEGIN") is True
    assert detect_secrets_in_logs("privatekey=xxx") is True

    # Should NOT detect safe logging
    assert detect_secrets_in_logs("Processing request ID: 12345") is False
    assert detect_secrets_in_logs("HTTP status code: 200") is False
    assert detect_secrets_in_logs("File path: /tmp/file.txt") is False
    assert detect_secrets_in_logs("User logged in successfully") is False
    # Note: Mentions of sensitive field names ARE flagged (conservative approach)
    # This is intentionally strict to avoid false negatives


def test_detect_secrets_case_insensitive():
    """Test that detection is case-insensitive."""
    from drep.security.detector import detect_secrets_in_logs

    assert detect_secrets_in_logs("TOKEN=abc123") is True
    assert detect_secrets_in_logs("Token=abc123") is True
    assert detect_secrets_in_logs("API_KEY=xyz") is True
    assert detect_secrets_in_logs("Password=secret") is True


def test_detect_secrets_in_urls():
    """Test that secrets in URLs are detected."""
    from drep.security.detector import detect_secrets_in_logs

    # URLs with tokens/keys should be detected
    assert detect_secrets_in_logs("http://api.com?token=abc123") is True
    assert detect_secrets_in_logs("http://api.com?api_key=secret") is True
    assert detect_secrets_in_logs("http://user:password@host.com") is True

    # Clean URLs should be safe
    assert detect_secrets_in_logs("http://api.com/endpoint") is False
    assert detect_secrets_in_logs("https://github.com/user/repo") is False


def test_detect_secrets_with_variable_names():
    """Test that logging variable names (without values) triggers detection."""
    from drep.security.detector import detect_secrets_in_logs

    # These patterns suggest sensitive variables are being logged
    assert detect_secrets_in_logs("Logging self.token") is True
    assert detect_secrets_in_logs("Debug: api_key value") is True
    assert detect_secrets_in_logs("password: {password}") is True


def test_sanitize_url_removes_secrets():
    """Test that sanitize_url masks secrets in URLs."""
    from drep.security.detector import sanitize_url

    # Token in query string
    assert sanitize_url("http://api.com?token=abc123") == "http://api.com?token=***"
    assert (
        sanitize_url("http://api.com?api_key=secret&page=1") == "http://api.com?api_key=***&page=1"
    )

    # OAuth tokens (CRITICAL - very common in API calls)
    assert sanitize_url("https://api.com?access_token=secret") == "https://api.com?access_token=***"
    assert sanitize_url("https://api.com?oauth_token=xyz") == "https://api.com?oauth_token=***"
    assert (
        sanitize_url("https://api.com?refresh_token=abc&page=1")
        == "https://api.com?refresh_token=***&page=1"
    )
    assert (
        sanitize_url("https://api.com?client_secret=secret") == "https://api.com?client_secret=***"
    )
    assert sanitize_url("https://api.com?client_id=12345") == "https://api.com?client_id=***"

    # Private keys
    assert (
        sanitize_url("https://api.com?private_key=-----BEGIN") == "https://api.com?private_key=***"
    )

    # Password in auth
    assert sanitize_url("http://user:password@host.com/path") == "http://user:***@host.com/path"

    # Multiple secrets
    assert sanitize_url("http://api.com?token=abc&key=xyz") == "http://api.com?token=***&key=***"

    # Clean URLs unchanged
    assert sanitize_url("http://api.com/endpoint") == "http://api.com/endpoint"
    assert sanitize_url("https://github.com/user/repo") == "https://github.com/user/repo"


def test_sanitize_url_handles_edge_cases():
    """Test sanitize_url with edge cases."""
    from drep.security.detector import sanitize_url

    # Empty/None
    assert sanitize_url("") == ""
    assert sanitize_url(None) == ""

    # No protocol
    assert sanitize_url("api.com?token=secret") == "api.com?token=***"

    # Fragment with token (rare but possible)
    assert sanitize_url("http://api.com#token=secret") == "http://api.com#token=***"


def test_get_secret_patterns():
    """Test that get_secret_patterns returns expected patterns."""
    from drep.security.detector import get_secret_patterns

    patterns = get_secret_patterns()

    # Should include key patterns
    assert any("token" in p.lower() for p in patterns)
    assert any("key" in p.lower() for p in patterns)
    assert any("password" in p.lower() for p in patterns)
    assert any("secret" in p.lower() for p in patterns)

    # All should be regex patterns (compile without error)
    import re

    for pattern in patterns:
        re.compile(pattern, re.IGNORECASE)  # Should not raise
