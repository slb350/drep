"""Unified diff parser for PR review."""

import re
from dataclasses import dataclass
from typing import List


@dataclass
class DiffHunk:
    """A single hunk from a unified diff."""

    file_path: str
    old_start: int
    old_count: int
    new_start: int
    new_count: int
    lines: List[str]  # Including +, -, and context lines (with prefixes)

    def get_added_lines(self) -> List[tuple[int, str]]:
        """Get only added lines with their line numbers in the new file.

        Returns:
            List of (line_number, content) tuples for added lines
        """
        added = []
        new_line = self.new_start

        for line in self.lines:
            if line.startswith("+") and not line.startswith("+++"):
                # Strip the + prefix for content
                content = line[1:]
                added.append((new_line, content))
                new_line += 1
            elif line.startswith(" "):
                # Context line increments new line number
                new_line += 1
            # Removed lines (-) don't increment new line number

        return added

    def get_removed_lines(self) -> List[tuple[int, str]]:
        """Get only removed lines with their line numbers in the old file.

        Returns:
            List of (line_number, content) tuples for removed lines
        """
        removed = []
        old_line = self.old_start

        for line in self.lines:
            if line.startswith("-") and not line.startswith("---"):
                # Strip the - prefix for content
                content = line[1:]
                removed.append((old_line, content))
                old_line += 1
            elif line.startswith(" "):
                # Context line increments old line number
                old_line += 1
            # Added lines (+) don't increment old line number

        return removed

    def get_context(self, context_lines: int = 3) -> str:
        """Get surrounding context for this hunk.

        Args:
            context_lines: Number of context lines to include (not used in basic implementation)

        Returns:
            String representation of the hunk with all lines
        """
        return "\n".join(self.lines)


def parse_diff(diff_text: str) -> List[DiffHunk]:
    """Parse unified diff into structured hunks.

    Args:
        diff_text: Unified diff string from Gitea

    Returns:
        List of DiffHunk objects

    Example:
        >>> diff = '''diff --git a/file.py b/file.py
        ... @@ -10,3 +10,4 @@ def func():
        ...  line1
        ... +line2
        ...  line3'''
        >>> hunks = parse_diff(diff)
        >>> len(hunks)
        1
    """
    if not diff_text or not diff_text.strip():
        return []

    hunks = []
    lines = diff_text.split("\n")
    current_file = None
    i = 0

    while i < len(lines):
        line = lines[i]

        # Detect file header: diff --git a/... b/...
        if line.startswith("diff --git "):
            # Extract file path from "diff --git a/path b/path"
            match = re.search(r"b/(.+)$", line)
            if match:
                current_file = match.group(1)
            i += 1
            continue

        # Detect hunk header: @@ -old_start,old_count +new_start,new_count @@
        if line.startswith("@@"):
            match = re.match(r"@@ -(\d+),?(\d*) \+(\d+),?(\d*) @@", line)
            if match and current_file:
                old_start = int(match.group(1))
                old_count = int(match.group(2)) if match.group(2) else 1
                new_start = int(match.group(3))
                new_count = int(match.group(4)) if match.group(4) else 1

                # Collect hunk lines (until next @@ or diff or end)
                hunk_lines = []
                i += 1

                while i < len(lines):
                    next_line = lines[i]

                    # Stop if we hit another hunk or file
                    if next_line.startswith("@@") or next_line.startswith("diff --git"):
                        break

                    # Skip file metadata lines (---, +++, index, etc.)
                    if next_line.startswith("---") or next_line.startswith("+++"):
                        i += 1
                        continue
                    if next_line.startswith("index ") or next_line.startswith("similarity "):
                        i += 1
                        continue
                    if next_line.startswith("rename ") or next_line.startswith("Binary files"):
                        i += 1
                        continue

                    # Include diff lines (+, -, space)
                    if next_line.startswith(("+", "-", " ")):
                        hunk_lines.append(next_line)

                    i += 1

                # Create hunk
                hunk = DiffHunk(
                    file_path=current_file,
                    old_start=old_start,
                    old_count=old_count,
                    new_start=new_start,
                    new_count=new_count,
                    lines=hunk_lines,
                )
                hunks.append(hunk)
            else:
                i += 1
        else:
            i += 1

    return hunks
