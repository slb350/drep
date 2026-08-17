"""Language support contract and the registry that resolves it.

drep analyzes a file in two layers, and this module is what keeps them free of
per-language conditionals:

- **Deterministic**: the project's own tools (ruff, eslint, gofmt, clippy).
  They are precise, so their findings can gate a commit.
- **Semantic**: the LLM, told which language it is looking at. It reads any
  language without a parser, so it needs no per-language machinery beyond a
  prompt - which is why adding a language here is a data change, not a
  refactor.

Deliberately free of heavyweight drep imports: the registry is consulted by
file discovery (``drep.core.file_targets``), which analyzer packages import.
"""

from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from drep.languages.prompts import build_analysis_prompt


@dataclass(frozen=True)
class ToolSpec:
    """A deterministic checker for one language.

    Attributes:
        name: Tool name, used in logs and finding provenance.
        command: argv to run, minus the files. The first element is resolved
            against local_paths before PATH.
        local_paths: Repo-relative locations to prefer over PATH, so a project
            gets the version its own CI runs (node_modules/.bin/eslint rather
            than whatever is installed globally).
        config_files: Repo-relative paths that mean "this project has opted
            into this tool". A tool with none of them present is skipped: its
            defaults are not the project's chosen style, so running it anyway
            would invent findings the project never asked for.
        output_format: How to parse the tool's diagnostics into findings.
        diagnostics_stream: Which stream carries them. `go vet` writes to
            stderr, so reading only stdout would report every Go file clean.
    """

    name: str
    command: tuple[str, ...]
    local_paths: tuple[str, ...]
    config_files: tuple[str, ...]
    output_format: str = "json"
    diagnostics_stream: str = "stdout"


@dataclass(frozen=True)
class LanguageSupport:
    """Everything drep needs to know about one language.

    Attributes:
        name: Registry key (lowercase, e.g. "typescript").
        display_name: How the language is named to the LLM and to users.
        extensions: Lowercased suffixes this language owns, including the dot.
        tools: Deterministic checkers, in the order they should run.
        conventions: Language-specific guidance appended to the analysis
            prompt - the part that used to be hardcoded as PEP 8.
    """

    name: str
    display_name: str
    extensions: tuple[str, ...]
    tools: tuple[ToolSpec, ...] = ()
    conventions: tuple[str, ...] = ()

    def analysis_prompt(self) -> str:
        """Code-quality prompt for this language.

        Shares its scaffolding and JSON schema with every other language, so
        response parsing stays language-independent; only the language name and
        the conventions differ.
        """
        return build_analysis_prompt(self)


class LanguageRegistry:
    """Extension -> language resolution.

    A registry rather than module-level dicts so tests can register a fake
    language without mutating global state that other tests observe.
    """

    def __init__(self) -> None:
        self._by_name: dict[str, LanguageSupport] = {}
        self._by_extension: dict[str, LanguageSupport] = {}

    def register(self, language: LanguageSupport) -> None:
        """Add a language. Raises if an extension is already claimed."""
        for extension in language.extensions:
            owner = self._by_extension.get(extension)
            if owner is not None and owner.name != language.name:
                raise ValueError(
                    f"{extension} is already owned by {owner.name}; "
                    f"{language.name} cannot claim it too"
                )
        self._by_name[language.name] = language
        for extension in language.extensions:
            self._by_extension[extension] = language

    def get(self, name: str) -> LanguageSupport:
        """Look up by registry key. Raises KeyError if unregistered."""
        return self._by_name[name]

    def detect(self, path: str | Path) -> LanguageSupport | None:
        """The language owning this path, or None if drep does not analyze it.

        Case-insensitive, matching the rest of drep's file-target policy.
        """
        name = path.name if isinstance(path, Path) else path
        _, dot, extension = name.rpartition(".")
        if not dot:
            return None
        return self._by_extension.get(f".{extension.lower()}")

    def languages(self) -> Iterable[LanguageSupport]:
        """Every registered language, in registration order."""
        return tuple(self._by_name.values())

    def source_extensions(self) -> frozenset[str]:
        """Every extension drep can analyze as source code."""
        return frozenset(self._by_extension)


registry = LanguageRegistry()
