"""Helpers for producing log output that is safe to keep.

Exception messages from HTTP clients routinely embed the request URL, which can
carry credentials (``?token=...``, ``https://user:pass@host``). Redaction lives
here, in one place, because it is security-relevant: three separately maintained
copies would drift, and a widened pattern would only reach some of them.
"""

import re

# Query-string / form credentials: token=…, api_key=…, password=…, secret=…
_CREDENTIAL_PARAM_RE = re.compile(r"(token|api_?key|password|secret)=[^&\s]+", re.IGNORECASE)

# Credentials embedded in a URL's authority: scheme://user:pass@host
_URL_USERINFO_RE = re.compile(r"://[^:]+:[^@]+@")


def sanitize_secrets(message: str) -> str:
    """Redact credentials that commonly appear inside error messages and URLs.

    Args:
        message: Raw message, typically ``str(exception)``

    Returns:
        The message with credential values replaced by ``***``

    Example:
        >>> sanitize_secrets("GET https://api/x?token=abc123 failed")
        'GET https://api/x?token=*** failed'
    """
    message = _CREDENTIAL_PARAM_RE.sub(r"\1=***", message)
    return _URL_USERINFO_RE.sub("://***:***@", message)
