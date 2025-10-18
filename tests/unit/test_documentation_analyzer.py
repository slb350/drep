"""Unit tests for DocumentationAnalyzer (Phase 3.7)."""

import pytest

from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.config import DocumentationConfig


@pytest.mark.asyncio
async def test_analyzer_initialization():
    """Test that DocumentationAnalyzer initializes with config."""
    config = DocumentationConfig(enabled=True, custom_dictionary=["gitea", "drep"])
    analyzer = DocumentationAnalyzer(config)

    assert analyzer.layer1 is not None
    assert analyzer.layer2 is not None


@pytest.mark.asyncio
async def test_analyze_python_file():
    """Test analyzing a Python file."""
    config = DocumentationConfig(enabled=True, custom_dictionary=["docstring"])
    analyzer = DocumentationAnalyzer(config)

    code = '''
def test():
    """Docstring with teh typo."""
    x  = 1  # Double  space
    return x
'''
    findings = await analyzer.analyze_file("test.py", code)

    assert findings.file_path == "test.py"
    # Should find typo in docstring
    assert len(findings.typos) > 0
    # Should find double space pattern
    assert len(findings.pattern_issues) > 0


@pytest.mark.asyncio
async def test_analyze_markdown_file():
    """Test analyzing a Markdown file."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[])
    analyzer = DocumentationAnalyzer(config)

    markdown = """# Title

This has teh typo  and double space.

```
Code block
```
"""
    findings = await analyzer.analyze_file("README.md", markdown)

    assert findings.file_path == "README.md"
    # Should find typo
    assert len(findings.typos) > 0
    # Should find double space
    assert len(findings.pattern_issues) > 0


@pytest.mark.asyncio
async def test_to_findings_conversion():
    """Test converting DocumentationFindings to generic Finding objects."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[])
    analyzer = DocumentationAnalyzer(config)

    text = "This has teh typo  and double space."
    doc_findings = await analyzer.analyze_file("test.txt", text)

    # Convert to generic findings
    findings = doc_findings.to_findings()

    # Should have findings from both layers
    assert len(findings) > 0
    # Verify finding structure
    for finding in findings:
        assert finding.file_path == "test.txt"
        assert finding.type in ["typo", "pattern"]
        assert finding.severity == "info"
        assert finding.line > 0
        assert finding.message


@pytest.mark.asyncio
async def test_custom_dictionary():
    """Test that custom dictionary is respected."""
    config = DocumentationConfig(enabled=True, custom_dictionary=["gitea", "drep"])
    analyzer = DocumentationAnalyzer(config)

    text = "The gitea and drep tools are great."
    findings = await analyzer.analyze_file("test.md", text)

    # Should not flag gitea or drep as typos
    typo_words = [t.word for t in findings.typos]
    assert "gitea" not in typo_words
    assert "drep" not in typo_words
