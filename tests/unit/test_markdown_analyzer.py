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


@pytest.mark.asyncio
async def test_markdown_missing_space_after_heading():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "#NoSpace\n##AlsoNoSpace\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "missing_space_after_heading"]
    assert len(issues) == 2
    assert issues[0].line == 1
    assert issues[1].line == 2


@pytest.mark.asyncio
async def test_markdown_multiple_blank_lines():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Line 1\n\n\n\nLine 2\n"  # 4 blank lines between Line 1 and Line 2
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "multiple_blank_lines"]
    assert len(issues) >= 1


@pytest.mark.asyncio
async def test_markdown_trailing_blank_lines():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Content\n\n"  # Trailing blank line at end
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "trailing_blank_lines"]
    assert len(issues) == 1
    assert issues[0].line == 2


@pytest.mark.asyncio
async def test_markdown_bare_url():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Visit https://example.com for more info\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "bare_url"]
    assert len(issues) == 1


@pytest.mark.asyncio
async def test_markdown_bare_url_ignores_proper_links():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Visit [our site](https://example.com) for more\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "bare_url"]
    assert len(issues) == 0  # Should not flag properly formatted links
