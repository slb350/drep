"""SpellcheckLayer - Layer 1: Dictionary spellcheck with context awareness."""

import re
from typing import List

from spellchecker import SpellChecker

from drep.models.findings import Typo


class SpellcheckLayer:
    """Layer 1: Dictionary spellcheck with context awareness."""

    def __init__(self, custom_words: List[str] = None):
        """Initialize SpellcheckLayer with optional custom dictionary.

        Args:
            custom_words: List of words to add to dictionary (e.g., ["gitea", "drep"])
        """
        self.spell = SpellChecker()
        if custom_words:
            self.spell.word_frequency.load_words(custom_words)

    def check(self, text: str, file_path: str = "") -> List[Typo]:
        """Check text for typos with context awareness.

        Args:
            text: The text content to check
            file_path: Path to the file (used to determine file type)

        Returns:
            List of Typo objects found in the text
        """
        if file_path.endswith(".md"):
            return self._check_markdown(text)
        elif file_path.endswith(".py"):
            return self._check_python_comments(text)
        else:
            return self._check_plain_text(text)

    def _check_line(self, line: str, line_num: int) -> List[Typo]:
        """Check a single line of text.

        Args:
            line: The line of text to check
            line_num: The line number (for reporting)

        Returns:
            List of Typo objects found in the line
        """
        typos = []

        # Remove URLs
        line_no_urls = re.sub(r"https?://\S+", "", line)

        # Remove inline code `like this`
        line_no_code = re.sub(r"`[^`]+`", "", line_no_urls)

        # Track occurrences of each word to handle duplicates
        word_occurrences = {}

        # Use finditer to get each word with its position
        # This allows us to detect duplicate typos on the same line
        for match in re.finditer(r"\b[a-zA-Z]+\b", line_no_code):
            word = match.group(0)

            # Skip if it looks like an identifier
            if self._is_identifier(word):
                continue

            # Check if misspelled
            if not self.spell.unknown([word]):
                continue  # Word is correctly spelled

            # Get suggestions
            suggestions = self.spell.candidates(word)
            replacement = list(suggestions)[0] if suggestions else word

            # Find column in ORIGINAL line (not cleaned line)
            # For duplicate words, find the Nth occurrence
            occurrence_index = word_occurrences.get(word, 0)
            word_occurrences[word] = occurrence_index + 1

            # Find the Nth occurrence of this word in the original line
            column = -1
            for i, m in enumerate(re.finditer(re.escape(word), line)):
                if i == occurrence_index:
                    column = m.start()
                    break

            if column == -1:
                # Fallback to first occurrence if something went wrong
                column = line.find(word)

            typos.append(
                Typo(
                    word=word,
                    replacement=replacement,
                    line=line_num,
                    column=column,
                    context=line.strip(),
                    suggestions=list(suggestions)[:5] if suggestions else [],
                )
            )

        return typos

    def _check_markdown(self, text: str) -> List[Typo]:
        """Check markdown, skipping code blocks.

        Args:
            text: The markdown text to check

        Returns:
            List of Typo objects found in markdown prose
        """
        typos = []
        lines = text.split("\n")
        in_code_block = False

        for line_num, line in enumerate(lines, 1):
            if line.strip().startswith("```"):
                in_code_block = not in_code_block
                continue

            if not in_code_block:
                # Check this line as prose
                line_typos = self._check_line(line, line_num)
                typos.extend(line_typos)

        return typos

    def _check_python_comments(self, text: str) -> List[Typo]:
        """Check Python comments and docstrings only.

        Args:
            text: The Python source code to check

        Returns:
            List of Typo objects found in comments and docstrings
        """
        import ast

        typos = []

        # Extract comments (lines with #)
        lines = text.split("\n")
        for line_num, line in enumerate(lines, 1):
            if "#" in line:
                comment = line.split("#", 1)[1]
                comment_typos = self._check_line(comment, line_num)
                typos.extend(comment_typos)

        # Extract docstrings using AST
        try:
            tree = ast.parse(text)
            for node in ast.walk(tree):
                # Only check nodes that can have docstrings
                if isinstance(
                    node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef, ast.Module)
                ):
                    docstring = ast.get_docstring(node)
                    if docstring:
                        # Get the function/class definition line
                        definition_line = node.lineno if hasattr(node, "lineno") else 1

                        # Check docstring for typos
                        doc_typos = self._check_plain_text(docstring)

                        # Adjust line numbers: _check_plain_text returns lines relative to
                        # the docstring (1, 2, 3...), but we need absolute source lines.
                        # The docstring starts after the def line, so add the offset.
                        for typo in doc_typos:
                            typo.line = definition_line + typo.line

                        typos.extend(doc_typos)
        except SyntaxError:
            pass  # Skip files with syntax errors

        return typos

    def _check_plain_text(self, text: str) -> List[Typo]:
        """Check plain text (for docstrings).

        Args:
            text: The plain text to check

        Returns:
            List of Typo objects found in the text
        """
        typos = []
        lines = text.split("\n")

        for line_num, line in enumerate(lines, 1):
            line_typos = self._check_line(line, line_num)
            typos.extend(line_typos)

        return typos

    def _is_identifier(self, word: str) -> bool:
        """Check if word looks like a code identifier.

        Args:
            word: The word to check

        Returns:
            True if word looks like a code identifier (camelCase, has numbers, etc.)
        """
        # camelCase
        if re.match(r"^[a-z]+[A-Z]", word):
            return True
        # Has numbers
        if re.search(r"\d", word):
            return True
        return False
