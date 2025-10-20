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

        # trailing whitespace + tabs + empty headings + long lines
        for idx, line in enumerate(lines, start=1):
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

            # Long lines (>120)
            if len(line) > 120:
                issues.append(
                    PatternIssue(
                        type="long_line",
                        line=idx,
                        column=121,
                        matched_text=line,
                        replacement="Wrap or rephrase to <=120 chars",
                    )
                )

        # Unclosed code fence: odd number of ```
        fence_count = sum(1 for l in lines if l.strip().startswith("```"))
        if fence_count % 2 == 1:
            # Find last fence line as location
            for idx in range(len(lines), 0, -1):
                if lines[idx - 1].strip().startswith("```"):
                    issues.append(
                        PatternIssue(
                            type="unclosed_code_fence",
                            line=idx,
                            column=1,
                            matched_text=lines[idx - 1],
                            replacement="```",  # Suggest closing fence
                        )
                    )
                    break

        return issues
