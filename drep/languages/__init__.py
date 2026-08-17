"""Per-language support: which files, which tools, which prompt.

Importing this package registers the built-in languages, so `registry` is
populated for every caller. Import from here rather than from `base` or
`definitions` directly - that ordering is the only reason the registry is
never observed half-built.
"""

# Imported for its side effect: registering the built-in languages.
from drep.languages import definitions as _definitions  # noqa: F401
from drep.languages.base import LanguageRegistry, LanguageSupport, ToolSpec, registry

__all__ = ["LanguageRegistry", "LanguageSupport", "ToolSpec", "registry"]
