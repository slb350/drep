"""DocumentationAnalyzer - Orchestrates documentation analysis.

Adds minimal Markdown checks (opt-in via config) and serves as a shim until
the LLM-based analysis is added.
"""

import re
from typing import List

from drep.models.config import DocumentationConfig
from drep.models.findings import DocumentationFindings, PatternIssue


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

        is_markdown = file_path.lower().endswith(".md")

        # Basic Markdown checks (opt-in)
        if is_markdown and getattr(self.config, "markdown_checks", False):
            findings.pattern_issues.extend(self._analyze_markdown(content))

        return findings

    def _analyze_markdown(self, content: str) -> List[PatternIssue]:
        issues: List[PatternIssue] = []
        lines = content.splitlines()

        # Track code fence state
        in_fence = False

        # Per-line checks: trailing whitespace, tabs, empty headings, missing space after heading, long lines
        for idx, line in enumerate(lines, start=1):
            stripped = line.rstrip("\n")
            if stripped.strip().startswith("```"):
                in_fence = not in_fence

            # Trailing whitespace
            if re.search(r"[ \t]+$", line):
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

            # Empty heading like '#   ' or '##'
            if re.match(r"^#{1,6}\s*$", line):
                level = len(line.split()[0]) if line.strip() else 1
                replacement = ("#" * level) + " Heading"
                issues.append(
                    PatternIssue(
                        type="empty_heading",
                        line=idx,
                        column=1,
                        matched_text=line,
                        replacement=replacement,
                    )
                )

            # Missing space after heading marker, e.g. '#Heading'
            if re.match(r"^#{1,6}\S", line):
                level = len(line) - len(line.lstrip("#"))
                replacement = ("#" * level) + " " + line[level:]
                issues.append(
                    PatternIssue(
                        type="missing_space_after_heading",
                        line=idx,
                        column=level + 1,
                        matched_text=line,
                        replacement=replacement,
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
        in_fence = False
        blank_run = 0
        for idx, line in enumerate(lines, start=1):
            if line.strip().startswith("```"):
                in_fence = not in_fence
                blank_run = 0
                continue
            if in_fence:
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
        in_fence = False
        for idx, line in enumerate(lines, start=1):
            if line.strip().startswith("```"):
                in_fence = not in_fence
                continue
            if in_fence:
                continue

            # Bare URL not wrapped in markdown link
            m = re.search(r"(?<!\()https?://\S+", line)
            if m and not re.search(r"\[[^\]]+\]\(https?://", line):
                issues.append(
                    PatternIssue(
                        type="bare_url",
                        line=idx,
                        column=m.start() + 1,
                        matched_text=line,
                        replacement="Wrap URL in [text](https://example.com)",
                    )
                )

            # Unmatched brackets/parentheses hinting broken link syntax
            if line.count("[") != line.count("]") or line.count("(") != line.count(")"):
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
        fence_count = sum(1 for l in lines if l.strip().startswith("```"))
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
