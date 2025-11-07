# Security Guidelines

This document outlines security practices for drep development, particularly around safe logging and secret handling.

## Safe Logging Practices

### ❌ NEVER Log These

**Absolutely forbidden:**
- API keys, tokens, passwords, secrets
- Authorization headers or Bearer tokens
- Full URLs containing sensitive query parameters
- Environment variables containing credentials
- SSH keys, certificates, or private keys
- Database connection strings with passwords
- User credentials or session tokens

### ✅ SAFE to Log

**Encouraged:**
- Request IDs and correlation IDs
- HTTP status codes (200, 404, 500, etc.)
- File paths (without user data)
- Timestamps and durations
- Error types (ValueError, KeyError) without values
- Sanitized URLs (use `sanitize_url()` helper)
- Count statistics (number of files, requests, etc.)

## Examples

### ❌ UNSAFE Logging

```python
# DON'T: Logs the token value
logger.info(f"Using token: {self.token}")

# DON'T: Logs URL with token
logger.info(f"Calling API: {url}")  # url = "http://api.com?token=secret"

# DON'T: Logs exception that might contain secrets
except httpx.HTTPStatusError as e:
    logger.error(f"Request failed: {e}")  # e.response might have tokens

# DON'T: Logs variable that might be sensitive
logger.debug(f"Config: {config}")  # config might have api_key
```

###  ✅ SAFE Logging

```python
# DO: Log without sensitive data
logger.info("Using authenticated API client")

# DO: Sanitize URLs before logging
from drep.security import sanitize_url
logger.info(f"Calling API: {sanitize_url(url)}")

# DO: Sanitize exception messages
except httpx.HTTPStatusError as e:
    error_msg = str(e)
    error_msg = re.sub(r"(token|api_key)=[^&\s]+", r"\1=***", error_msg, flags=re.IGNORECASE)
    logger.error(f"Request failed: {error_msg}")

# DO: Log status/type without values
logger.info(f"HTTP {response.status_code}")
logger.error(f"Failed to parse response: {type(e).__name__}")
```

## Security Utilities

### `detect_secrets_in_logs(log_line: str) -> bool`

Detect if a log message contains potential secrets.

```python
from drep.security import detect_secrets_in_logs

if detect_secrets_in_logs(message):
    raise SecurityError("Attempted to log sensitive data!")
```

### `sanitize_url(url: str) -> str`

Sanitize URLs by masking tokens, keys, and passwords.

```python
from drep.security import sanitize_url

# Before: "http://api.com?token=abc123&page=1"
# After:  "http://api.com?token=***&page=1"
safe_url = sanitize_url(dangerous_url)
logger.info(f"Fetching: {safe_url}")
```

## Audit Checklist

When adding new logging statements:

1. **Variable Names**
   - [ ] No variables named `token`, `key`, `password`, `secret`, `auth`
   - [ ] No `self.token` or similar attribute references
   - [ ] No config objects that might contain secrets

2. **URLs**
   - [ ] All URLs sanitized with `sanitize_url()` before logging
   - [ ] No query parameters like `?token=xxx` or `?api_key=xxx`

3. **Exception Messages**
   - [ ] HTTP exceptions sanitized (might contain URLs)
   - [ ] ValueError from API calls sanitized
   - [ ] No raw exception objects logged

4. **Headers**
   - [ ] No Authorization headers logged
   - [ ] No Cookie headers logged
   - [ ] No custom headers containing tokens

## Code Review

Before committing, review all new/modified logging statements:

```bash
# Find all logger calls in your changes
git diff --cached | grep -i "logger\."

# Check for sensitive variable names
git diff --cached | grep -iE "(token|api_key|password|secret)\s*="
```

## Incident Response

If secrets are accidentally logged:

1. **Immediate**: Revoke the exposed credentials
2. **Rotate**: Generate new credentials
3. **Audit**: Check if logs were accessed/exported
4. **Fix**: Add sanitization or remove the logging statement
5. **Review**: Update this document with new patterns

## Automated Checks

### Pre-commit Hook (Future)

A pre-commit hook will scan for secret patterns:

```bash
# .pre-commit-hooks/check-secrets.py
# Blocks commits with logging of sensitive data
```

### CI/CD Checks (Future)

GitHub Actions will flag potential security issues:

```yaml
# .github/workflows/security-check.yml
# Runs secret detection on all code changes
```

## References

- [OWASP Logging Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html)
- [CWE-532: Insertion of Sensitive Information into Log File](https://cwe.mitre.org/data/definitions/532.html)

## Questions?

If unsure whether something is safe to log, ask:
- "Could this contain user credentials?"
- "Could this expose API keys or tokens?"
- "Would I be comfortable with this in public logs?"

When in doubt, **don't log it** or **sanitize it first**.
