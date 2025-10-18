"""PatternLayer - Layer 2: Pattern matching for common issues."""

import re
from typing import List

from drep.models.findings import PatternIssue


class PatternLayer:
    """Layer 2: Pattern matching for common issues."""

    PATTERNS = {
        # Double space pattern: 2+ spaces followed by non-whitespace (not at end of line)
        "double_space": (r"  +(?=\S)", " "),
        # Trailing whitespace: spaces at end of line
        "trailing_whitespace": (r" +$", ""),
    }

    def check(self, text: str, file_ext: str) -> List[PatternIssue]:
        """Check text for pattern issues.

        Args:
            text: The text content to check
            file_ext: File extension (not currently used, but available for future filtering)

        Returns:
            List of PatternIssue objects found in the text
        """
        issues = []

        for pattern_name, (regex, replacement) in self.PATTERNS.items():
            for match in re.finditer(regex, text, re.MULTILINE):
                # Find line number
                line_num = text[: match.start()].count("\n") + 1
                col_num = match.start() - text.rfind("\n", 0, match.start()) - 1

                issues.append(
                    PatternIssue(
                        type=pattern_name,
                        line=line_num,
                        column=col_num,
                        matched_text=match.group(0),
                        replacement=replacement,
                    )
                )

        return issues
