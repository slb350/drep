"""DocumentationAnalyzer - Orchestrates documentation analysis.

Adds minimal Markdown checks (opt-in via config) and serves as a shim until
the LLM-based analysis is added.
"""

import re

from drep.core.file_targets import is_markdown
from drep.models.config import DocumentationConfig
from drep.models.findings import DocumentationFindings, PatternIssue

# Compiled once each: every one of these runs per line of every file scanned.
_INLINE_CODE = re.compile(r"`[^`]*`")  # inline code span: `like this`
_HEADING_EMPTY = re.compile(r"^#{1,6}\s*$")  # '#   ' or '##'
# '#Heading'. The class must exclude '#' itself: \S lets the regex backtrack and
# match the second '#' of a well-formed '## Heading'.
_HEADING_NO_SPACE = re.compile(r"^#{1,6}[^#\s]")
_BARE_URL = re.compile(r"https?://\S+")
# A complete [text](url). The link text allows one level of nested brackets so
# the badge construct `[![alt](img)](href)` is matched whole; `[^\]]*` stopped
# at the image's own `]` and left `](href)` looking like broken syntax.
_MARKDOWN_LINK = re.compile(r"\[(?:[^\[\]]|\[[^\]]*\])*\]\([^)]*\)")


def _blank(match: re.Match[str]) -> str:
    """Same-length blanks, so blanking a span leaves every column intact."""
    return " " * len(match.group())


def _is_fence_delimiter(line: str) -> bool:
    """The one test for a fence delimiter, so refining it is a single edit."""
    return line.strip().startswith("```")


def _fence_mask(lines: list[str]) -> tuple[list[bool], list[int]]:
    """Fence state per line, plus the 1-based line numbers of the delimiters.

    Derived once instead of per check: every check that forgot to consult its
    loop's private `in_fence` toggle became a false positive on code samples.
    Returning the delimiters too keeps the unclosed-fence check from scanning
    for them a second time.
    """
    mask: list[bool] = []
    delimiters: list[int] = []
    in_fence = False
    for idx, line in enumerate(lines, start=1):
        if _is_fence_delimiter(line):
            delimiters.append(idx)
            in_fence = not in_fence
            mask.append(True)
        else:
            mask.append(in_fence)
    return mask, delimiters


class DocumentationAnalyzer:
    """Orchestrates documentation analysis.

    - Markdown checks (when enabled):
      - trailing_whitespace
      - empty_heading (e.g., '#   ')
      - unclosed_code_fence (odd number of ```)
      - tab_character (\t present)
      - long_line (> 120 chars)
    """

    def __init__(self, config: DocumentationConfig):
        self.config = config

    async def analyze_file(self, file_path: str, content: str) -> DocumentationFindings:
        findings = DocumentationFindings(file_path=file_path)

        if not self.config.enabled:
            return findings

        # Basic Markdown checks (opt-in)
        if is_markdown(file_path) and getattr(self.config, "markdown_checks", False):
            findings.pattern_issues.extend(self._analyze_markdown(content))

        return findings

    def _analyze_markdown(self, content: str) -> list[PatternIssue]:
        issues: list[PatternIssue] = []
        lines = content.splitlines()
        fenced, fence_delimiters = _fence_mask(lines)

        # Per-line checks: trailing whitespace, tabs, empty headings,
        # missing space after heading, long lines
        for idx, (line, in_fence) in enumerate(zip(lines, fenced, strict=True), start=1):
            # Trailing whitespace
            if line != line.rstrip(" \t"):
                issues.append(
                    PatternIssue(
                        type="trailing_whitespace",
                        line=idx,
                        column=len(line.rstrip()) + 1,
                        matched_text=line,
                        replacement=line.rstrip(),
                    )
                )

            # Tab characters
            if "\t" in line:
                issues.append(
                    PatternIssue(
                        type="tab_character",
                        line=idx,
                        column=(line.find("\t") + 1),
                        matched_text=line,
                        replacement=line.replace("\t", "    "),
                    )
                )

            # Heading checks. Guarded on the cheap prefix test first: most lines
            # cannot be headings, and the regexes are the bulk of this loop.
            if not in_fence and line.startswith("#"):
                level = len(line) - len(line.lstrip("#"))

                # Empty heading like '#   ' or '##'
                if _HEADING_EMPTY.match(line):
                    issues.append(
                        PatternIssue(
                            type="empty_heading",
                            line=idx,
                            column=1,
                            matched_text=line,
                            replacement=("#" * level) + " Heading",
                        )
                    )

                # Missing space after heading marker, e.g. '#Heading'
                elif _HEADING_NO_SPACE.match(line):
                    issues.append(
                        PatternIssue(
                            type="missing_space_after_heading",
                            line=idx,
                            column=level + 1,
                            matched_text=line,
                            replacement=("#" * level) + " " + line[level:],
                        )
                    )

            # Long lines (>120) - skip inside code fences
            if not in_fence and len(line) > 120:
                issues.append(
                    PatternIssue(
                        type="long_line",
                        line=idx,
                        column=121,
                        matched_text=line,
                        replacement="Wrap or rephrase to <=120 chars",
                    )
                )

        # Multiple blank lines (>=3) outside code fences
        blank_run = 0
        for idx, (line, in_fence) in enumerate(zip(lines, fenced, strict=True), start=1):
            if in_fence:
                blank_run = 0
                continue
            if line.strip() == "":
                blank_run += 1
                if blank_run == 3:
                    issues.append(
                        PatternIssue(
                            type="multiple_blank_lines",
                            line=idx - 2,
                            column=1,
                            matched_text="",
                            replacement="Reduce consecutive blank lines",
                        )
                    )
            else:
                blank_run = 0

        # Trailing blank lines at end of file
        if len(lines) > 0 and lines[-1].strip() == "":
            issues.append(
                PatternIssue(
                    type="trailing_blank_lines",
                    line=len(lines),
                    column=1,
                    matched_text="",
                    replacement="Remove trailing blank lines",
                )
            )

        # Basic link syntax checks and bare URL detection (outside fences)
        for idx, (line, in_fence) in enumerate(zip(lines, fenced, strict=True), start=1):
            if in_fence:
                continue

            # Inline code is a literal to type, not prose: a URL or a stray
            # bracket inside backticks is neither a link nor broken syntax.
            # Blanked rather than removed so columns still match the real line.
            uncoded = _INLINE_CODE.sub(_blank, line) if "`" in line else line

            # Well-formed links blanked out once, then reused by both checks
            # below. Guarded on the cheap substring test: most lines hold no
            # link at all, and sub() copies the whole string regardless.
            unlinked = _MARKDOWN_LINK.sub(_blank, uncoded) if "](" in uncoded else uncoded

            # A bare URL is whatever URL survives that blanking - one link on
            # the line must not excuse every bare URL beside it.
            m = _BARE_URL.search(unlinked)
            if m:
                issues.append(
                    PatternIssue(
                        type="bare_url",
                        line=idx,
                        column=m.start() + 1,
                        matched_text=line,
                        replacement="Wrap URL in [text](https://example.com)",
                    )
                )

            # Brackets or parens left unbalanced *after* the well-formed links
            # are removed. Counting over the raw line flags every prose
            # parenthetical that wraps onto a second line.
            if unlinked.count("[") != unlinked.count("]") or (
                "](" in unlinked and unlinked.count("(") != unlinked.count(")")
            ):
                issues.append(
                    PatternIssue(
                        type="link_syntax_invalid",
                        line=idx,
                        column=1,
                        matched_text=line,
                        replacement="Fix markdown link syntax [text](url)",
                    )
                )

        # Unclosed code fence: an odd number of delimiters leaves the last one open
        if len(fence_delimiters) % 2 == 1:
            idx = fence_delimiters[-1]
            issues.append(
                PatternIssue(
                    type="unclosed_code_fence",
                    line=idx,
                    column=1,
                    matched_text=lines[idx - 1],
                    replacement="```",
                )
            )

        return issues
