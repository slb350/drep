"""Single source of truth for which files drep analyzes.

All discovery and filter paths go through these predicates so every workflow
(full scan, commit diff, staged files, per-analyzer filters) makes identical,
case-insensitive decisions.

This module deliberately has no drep imports: analyzer packages
(``drep.code_quality``, ``drep.documentation``) need the same policy as
``drep.core.scanner``, which imports those packages in turn.
"""

import os
from collections.abc import Callable, Iterable, Iterator
from pathlib import Path

SCAN_TARGET_SUFFIXES = frozenset({".py", ".md"})
PYTHON_SOURCE_SUFFIXES = frozenset({".py"})
MARKDOWN_SUFFIXES = frozenset({".md"})

# Directory names never descended into during discovery. Module-level so the set
# is built once rather than per candidate file.
IGNORED_DIRS = frozenset(
    {
        "__pycache__",
        ".git",
        "venv",
        "env",
        ".venv",
        ".tox",
        "build",
        "dist",
        ".eggs",
    }
)


def _suffix_of(path: str | Path) -> str:
    """Lowercased file extension, without constructing a Path for str inputs."""
    name = path.name if isinstance(path, Path) else path
    _, dot, ext = name.rpartition(".")
    return f".{ext.lower()}" if dot else ""


def is_scan_target(path: str | Path) -> bool:
    """Return True if the path is a file type drep analyzes (.py/.md, case-insensitive)."""
    return _suffix_of(path) in SCAN_TARGET_SUFFIXES


def is_python_source(path: str | Path) -> bool:
    """Return True if the path is a Python source file (case-insensitive)."""
    return _suffix_of(path) in PYTHON_SOURCE_SUFFIXES


def is_markdown(path: str | Path) -> bool:
    """Return True if the path is a Markdown document (case-insensitive)."""
    return _suffix_of(path) in MARKDOWN_SUFFIXES


def is_ignored_dir(name: str) -> bool:
    """Return True if a directory component should never be descended into.

    Case-insensitive like the suffix predicates: this module promises identical
    decisions, and on a case-insensitive filesystem `VENV` and `venv` are the
    same directory - matching only one of them means walking it anyway.
    """
    folded = name.casefold()
    return folded in IGNORED_DIRS or folded.endswith(".egg-info")


def expand_paths(paths: Iterable[str | Path], predicate: Callable[[str], bool]) -> list[Path]:
    """Expand a mix of files and directories into the matching files, deduped.

    Deduped because the same file can be named twice - `drep check a.py .` -
    and a duplicate costs a whole extra LLM round-trip, not just a repeated
    line of output. Explicit filenames are filtered by the same predicate as
    walked ones, so naming a file cannot smuggle in a type drep does not read.
    """
    found: set[Path] = set()
    for raw in paths:
        path_obj = Path(raw)
        if path_obj.is_dir():
            found.update(walk_targets(path_obj, predicate))
        elif predicate(path_obj.name):
            found.add(path_obj)
    return sorted(found)


def walk_targets(root: str | Path, predicate: Callable[[str], bool]) -> Iterator[Path]:
    """Yield files under ``root`` matching ``predicate``, skipping ignored trees.

    os.walk with in-place pruning so ignored trees (.git, venv, build, …) are
    never descended into. rglob("*") would stat every entry in them first -
    tens of thousands of wasted syscalls on a cloned repo.
    """
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if not is_ignored_dir(d)]
        base = Path(dirpath)
        for name in filenames:
            if predicate(name):
                yield base / name
