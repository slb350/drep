"""Unit tests for DocumentationAnalyzer.

Note: Legacy spellcheck/pattern tests removed in Phase 7.0.
These tests now verify basic analyzer functionality.
LLM-based tests will be added in Phase 7.2.
"""

import pytest

from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.config import DocumentationConfig


@pytest.mark.asyncio
async def test_analyzer_initialization():
    """Test that DocumentationAnalyzer initializes with config."""
    config = DocumentationConfig(enabled=True, custom_dictionary=["gitea", "drep"])
    analyzer = DocumentationAnalyzer(config)

    assert analyzer.config is not None
    assert analyzer.config.enabled is True


@pytest.mark.asyncio
async def test_analyze_python_file():
    """Test analyzing a Python file returns empty findings for now."""
    config = DocumentationConfig(enabled=True, custom_dictionary=["docstring"])
    analyzer = DocumentationAnalyzer(config)

    code = '''
def test():
    """Docstring with text."""
    x = 1
    return x
'''
    findings = await analyzer.analyze_file("test.py", code)

    assert findings.file_path == "test.py"
    # Returns empty findings until Phase 7.2 LLM integration
    assert len(findings.typos) == 0
    assert len(findings.pattern_issues) == 0


@pytest.mark.asyncio
async def test_analyze_markdown_file():
    """Test analyzing a Markdown file returns empty findings for now."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[])
    analyzer = DocumentationAnalyzer(config)

    markdown = """# Title

This is some text.

```
Code block
```
"""
    findings = await analyzer.analyze_file("README.md", markdown)

    assert findings.file_path == "README.md"
    # Returns empty findings until Phase 7.2 LLM integration
    assert len(findings.typos) == 0
    assert len(findings.pattern_issues) == 0


@pytest.mark.asyncio
async def test_to_findings_conversion():
    """Test converting DocumentationFindings to generic Finding objects."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[])
    analyzer = DocumentationAnalyzer(config)

    text = "This is some text."
    doc_findings = await analyzer.analyze_file("test.txt", text)

    # Convert to generic findings
    findings = doc_findings.to_findings()

    # Should have no findings until Phase 7.2 LLM integration
    assert len(findings) == 0
