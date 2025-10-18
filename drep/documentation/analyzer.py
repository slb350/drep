"""DocumentationAnalyzer - Orchestrates documentation analysis.

Legacy spellcheck and pattern layers removed in Phase 7.0.
Will be replaced with LLM-based analysis in Phase 7.2.
"""

from drep.models.config import DocumentationConfig
from drep.models.findings import DocumentationFindings


class DocumentationAnalyzer:
    """Orchestrates documentation analysis.

    Note: Legacy tiered analysis (spellcheck/patterns) removed.
    LLM-based analysis will be added in Phase 7.2.
    """

    def __init__(self, config: DocumentationConfig):
        """Initialize DocumentationAnalyzer with config.

        Args:
            config: DocumentationConfig with enabled status
        """
        self.config = config

    async def analyze_file(self, file_path: str, content: str) -> DocumentationFindings:
        """Run analysis on a file.

        Args:
            file_path: Path to the file (used for routing and reporting)
            content: The file content to analyze

        Returns:
            DocumentationFindings (empty for now, LLM analysis in Phase 7.2)
        """
        # Return empty findings for now
        # LLM-based analysis will be added in Phase 7.2
        findings = DocumentationFindings(file_path=file_path)
        return findings
