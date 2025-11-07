"""Secret detection utilities for safe logging.

This module provides utilities to detect and sanitize sensitive information
in log messages, preventing accidental exposure of API keys, tokens, passwords,
and other credentials.
"""

import re
from typing import List, Optional
from urllib.parse import urlparse, urlunparse


def get_secret_patterns() -> List[str]:
    """Get regex patterns for detecting secrets in log messages.

    Returns:
        List of regex pattern strings (case-insensitive matching)

    Patterns detect:
    - Variable assignments: token=value, api_key=value
    - HTTP headers: Authorization: Bearer xxx
    - Standalone Bearer tokens: Bearer abc123
    - OAuth tokens: access_token, oauth_token, refresh_token, client_secret
    - URL components: ?token=xxx, user:password@host
    - Variable names in messages: self.token, {password}
    - Private keys: private_key=xxx
    """
    return [
        # Assignment patterns: token=xxx, api_key=xxx, access_token=xxx
        r"\b(?:token|api_?key|password|secret|auth|access_token|oauth_token|refresh_token|client_secret|client_id|private_?key)\s*[=:]",
        # Authorization header
        r"authorization\s*:\s*bearer",
        # Standalone Bearer tokens
        r"\bbearer\s+[a-zA-Z0-9_\-]+",
        # URL with sensitive params (including OAuth)
        r"[?&](?:token|api_?key|key|password|secret|access_token|oauth_token|refresh_token|client_secret|client_id)=",
        # Password in URL auth
        r"://[^:]+:[^@]+@",
        # Variable references that suggest secrets
        r"(?:self\.|this\.)?(?:token|api_?key|password|secret|access_token|oauth_token|refresh_token|client_secret|private_?key)\b",
        # String formatting with sensitive vars
        r"\{(?:token|api_?key|password|secret|access_token|oauth_token|refresh_token|client_secret|private_?key)\}",
    ]


def detect_secrets_in_logs(log_line: Optional[str]) -> bool:
    """Detect if a log line contains secrets or sensitive information.

    Args:
        log_line: Log message to check (None-safe)

    Returns:
        True if log line appears to contain secrets, False otherwise

    Example:
        >>> detect_secrets_in_logs("Authorization: Bearer abc123")
        True
        >>> detect_secrets_in_logs("Processing request ID: 12345")
        False
    """
    if not log_line:
        return False

    patterns = get_secret_patterns()

    for pattern in patterns:
        if re.search(pattern, log_line, re.IGNORECASE):
            return True

    return False


def sanitize_url(url: Optional[str]) -> str:
    """Sanitize URL by masking sensitive query parameters and auth credentials.

    Args:
        url: URL to sanitize (None-safe)

    Returns:
        Sanitized URL with secrets replaced by '***'

    Example:
        >>> sanitize_url("http://api.com?token=abc123")
        'http://api.com?token=***'
        >>> sanitize_url("http://user:password@host.com")
        'http://user:***@host.com'
    """
    if not url:
        return ""

    # Sensitive parameter names to mask
    sensitive_params = {
        "token",
        "api_key",
        "apikey",
        "key",
        "password",
        "secret",
        "auth",
        "authorization",
        # OAuth tokens (CRITICAL - commonly used in API calls)
        "access_token",
        "oauth_token",
        "refresh_token",
        "client_secret",
        "client_id",
        # SSH/Private keys
        "private_key",
        "privatekey",
    }

    try:
        # Parse URL
        parsed = urlparse(url)

        # Mask password in netloc (user:password@host)
        netloc = parsed.netloc
        if "@" in netloc and ":" in netloc.split("@")[0]:
            user_pass, host = netloc.rsplit("@", 1)
            if ":" in user_pass:
                user, _ = user_pass.split(":", 1)
                netloc = f"{user}:***@{host}"

        # Mask sensitive query parameters
        if parsed.query:
            query_parts = []
            for param in parsed.query.split("&"):
                if "=" in param:
                    key, value = param.split("=", 1)
                    if key.lower().replace("_", "") in {
                        p.replace("_", "") for p in sensitive_params
                    }:
                        # Mask the value
                        query_parts.append(f"{key}=***")
                    else:
                        # Keep original
                        query_parts.append(param)
                else:
                    query_parts.append(param)
            query = "&".join(query_parts)
        else:
            query = parsed.query

        # Mask sensitive fragment parameters (rare but possible)
        fragment = parsed.fragment
        if fragment and any(f"{param}=" in fragment.lower() for param in sensitive_params):
            # Simple replacement for fragments
            for param in sensitive_params:
                fragment = re.sub(
                    rf"({param}=)[^&\s]+",
                    r"\1***",
                    fragment,
                    flags=re.IGNORECASE,
                )

        # Reconstruct URL
        return urlunparse((parsed.scheme, netloc, parsed.path, parsed.params, query, fragment))

    except Exception:
        # If URL parsing fails, try simple regex replacement as fallback
        sanitized = url
        for param in sensitive_params:
            # Replace param=value patterns
            sanitized = re.sub(
                rf"({param}=)[^&\s]+",
                r"\1***",
                sanitized,
                flags=re.IGNORECASE,
            )
        return sanitized
