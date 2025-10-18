"""DocumentationAnalyzer - Orchestrates tiered documentation analysis."""

from pathlib import Path

from drep.documentation.patterns import PatternLayer
from drep.documentation.spellcheck import SpellcheckLayer
from drep.models.config import DocumentationConfig
from drep.models.findings import DocumentationFindings


class DocumentationAnalyzer:
    """Orchestrates tiered documentation analysis."""

    def __init__(self, config: DocumentationConfig):
        """Initialize DocumentationAnalyzer with config.

        Args:
            config: DocumentationConfig with enabled status and custom dictionary
        """
        self.layer1 = SpellcheckLayer(custom_words=config.custom_dictionary)
        self.layer2 = PatternLayer()

    async def analyze_file(self, file_path: str, content: str) -> DocumentationFindings:
        """Run tiered analysis on a file.

        Args:
            file_path: Path to the file (used for routing and reporting)
            content: The file content to analyze

        Returns:
            DocumentationFindings with typos and pattern issues
        """
        findings = DocumentationFindings(file_path=file_path)

        # Layer 1: Spellcheck (PASS file_path for context)
        typos = self.layer1.check(content, file_path=file_path)
        findings.typos = typos

        # Layer 2: Pattern matching
        file_ext = Path(file_path).suffix.lstrip(".")
        pattern_issues = self.layer2.check(content, file_ext)
        findings.pattern_issues = pattern_issues

        return findings
