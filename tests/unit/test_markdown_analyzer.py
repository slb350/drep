"""Tests for Markdown checks in DocumentationAnalyzer."""

import pytest

from drep.documentation.analyzer import DocumentationAnalyzer
from drep.models.config import DocumentationConfig


@pytest.mark.asyncio
async def test_markdown_trailing_whitespace_detection():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Line with space  \nClean\n"
    findings = await analyzer.analyze_file("README.md", md)

    issues = [i for i in findings.pattern_issues if i.type == "trailing_whitespace"]
    assert len(issues) == 1
    assert issues[0].line == 1


@pytest.mark.asyncio
async def test_markdown_empty_heading_detection():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "#   \nSome text\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "empty_heading"]
    assert len(issues) == 1
    assert issues[0].line == 1


@pytest.mark.asyncio
async def test_markdown_tab_character_detection():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Line\twith\ttabs\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "tab_character"]
    assert len(issues) >= 1


@pytest.mark.asyncio
async def test_markdown_long_line_detection():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    long = "a" * 121
    md = f"{long}\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "long_line"]
    assert len(issues) == 1
    assert issues[0].line == 1


@pytest.mark.asyncio
async def test_markdown_unclosed_code_fence_detection():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = """Intro

```
code block
"""
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "unclosed_code_fence"]
    assert len(issues) == 1
