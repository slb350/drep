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
_MARKDOWN_LINK = re.compile(r"\[[^\]]*\]\([^)]*\)")  # a complete [text](url)


def _blank(match: re.Match[str]) -> str:
    """Same-length blanks, so blanking a span leaves every column intact."""
    return " " * len(match.group())


def _fence_mask(lines: list[str]) -> list[bool]:
    """One answer to "is this line code?" - True for fenced lines and delimiters.

    Derived once instead of per check: every check that forgot to consult its
    loop's private `in_fence` toggle became a false positive on code samples.
    """
    mask: list[bool] = []
    in_fence = False
    for line in lines:
        is_delimiter = line.strip().startswith("```")
        if is_delimiter:
            in_fence = not in_fence
        mask.append(in_fence or is_delimiter)
    return mask


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
        fenced = _fence_mask(lines)

        # Per-line checks: trailing whitespace, tabs, empty headings,
        # missing space after heading, long lines
        for idx, line in enumerate(lines, start=1):
            in_fence = fenced[idx - 1]

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
                # Empty heading like '#   ' or '##'
                if _HEADING_EMPTY.match(line):
                    # The regex only matches a run of '#', so split() is never empty
                    level = len(line.split()[0])
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
                    level = len(line) - len(line.lstrip("#"))
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
        for idx, line in enumerate(lines, start=1):
            if fenced[idx - 1]:
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
        for idx, line in enumerate(lines, start=1):
            if fenced[idx - 1]:
                continue

            # Inline code is a literal to type, not prose: a URL or a stray
            # bracket inside backticks is neither a link nor broken syntax.
            # Blanked rather than removed so columns still match the real line.
            uncoded = _INLINE_CODE.sub(_blank, line) if "`" in line else line

            # Bare URL not wrapped in a markdown link. Blank the well-formed
            # links and look at what is left, rather than letting one link on
            # the line excuse every bare URL beside it.
            m = _BARE_URL.search(_MARKDOWN_LINK.sub(_blank, uncoded))
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

            # Unmatched brackets, or unmatched parens in a line that actually
            # holds a link. Bare paren counting flags every prose parenthetical
            # that wraps onto a second line.
            if uncoded.count("[") != uncoded.count("]") or (
                "](" in uncoded and uncoded.count("(") != uncoded.count(")")
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

        # Unclosed code fence: odd number of ```
        fence_count = sum(1 for line in lines if line.strip().startswith("```"))
        if fence_count % 2 == 1:
            for idx in range(len(lines), 0, -1):
                if lines[idx - 1].strip().startswith("```"):
                    issues.append(
                        PatternIssue(
                            type="unclosed_code_fence",
                            line=idx,
                            column=1,
                            matched_text=lines[idx - 1],
                            replacement="```",
                        )
                    )
                    break

        return issues
