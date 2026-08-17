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
async def test_markdown_well_formed_headings_are_not_flagged():
    """The second '#' of a sub-heading is not a missing space.

    A regex of ^#{1,6}\\S backtracks to one '#' and matches the next '#',
    flagging every correct heading below level 1 - 65 false positives in this
    project's own README alone.
    """
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "# Title\n## Features\n### Details\n###### Deep\n"
    findings = await analyzer.analyze_file("README.md", md)
    issues = [i for i in findings.pattern_issues if i.type == "missing_space_after_heading"]
    assert issues == []


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


@pytest.mark.asyncio
async def test_markdown_heading_checks_skip_code_fences():
    """Shell and Python comments in samples are code, not headings.

    Every README with a bash block starts one with `#!/bin/bash`; flagging it
    as a malformed heading makes the report unusable as a gate.
    """
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "# Title\n\n```bash\n#!/bin/bash\n#comment\n##\n```\n"
    findings = await analyzer.analyze_file("README.md", md)

    heading_types = {"missing_space_after_heading", "empty_heading"}
    assert [i for i in findings.pattern_issues if i.type in heading_types] == []


@pytest.mark.asyncio
async def test_markdown_prose_parenthetical_is_not_broken_link_syntax():
    """A parenthetical spanning two lines is prose, not a malformed link."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Webhooks are accepted (any caller who can reach\nthe endpoint can post).\n"
    findings = await analyzer.analyze_file("README.md", md)

    assert [i for i in findings.pattern_issues if i.type == "link_syntax_invalid"] == []


@pytest.mark.asyncio
async def test_markdown_broken_link_syntax_still_detected():
    """Narrowing the paren rule must not blind the check to real breakage."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "See [the docs](https://example.com for details.\n"
    findings = await analyzer.analyze_file("README.md", md)

    assert [i for i in findings.pattern_issues if i.type == "link_syntax_invalid"] != []


@pytest.mark.asyncio
async def test_markdown_url_in_inline_code_is_not_a_bare_url():
    """A URL in backticks is a literal to type, not a link to wrap."""
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Point the webhook at `http://your-server:8000/webhooks/gitea` to start.\n"
    findings = await analyzer.analyze_file("README.md", md)

    assert [i for i in findings.pattern_issues if i.type == "bare_url"] == []


@pytest.mark.asyncio
async def test_markdown_bare_url_outside_code_still_detected():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "Read more at https://example.com/docs today.\n"
    findings = await analyzer.analyze_file("README.md", md)

    issues = [i for i in findings.pattern_issues if i.type == "bare_url"]
    assert len(issues) == 1
    assert issues[0].column == md.index("https://") + 1


@pytest.mark.asyncio
async def test_markdown_bare_url_alongside_a_proper_link_is_still_flagged():
    """A well-formed link on the line must not excuse a bare URL next to it.

    The check bailed whenever the line contained any markdown link, so
    '[docs](https://a) see https://b' reported nothing.
    """
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "See [the docs](https://example.com) or just https://example.org/raw\n"
    findings = await analyzer.analyze_file("README.md", md)

    issues = [i for i in findings.pattern_issues if i.type == "bare_url"]
    assert len(issues) == 1
    assert issues[0].column == md.index("https://example.org") + 1


@pytest.mark.asyncio
async def test_markdown_url_inside_a_link_is_not_a_bare_url():
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "See [the docs](https://example.com) for details.\n"
    findings = await analyzer.analyze_file("README.md", md)

    assert [i for i in findings.pattern_issues if i.type == "bare_url"] == []


@pytest.mark.asyncio
async def test_markdown_image_inside_a_link_is_valid_syntax():
    """`[![alt](img)](href)` is the standard badge construct, not broken syntax.

    A link-text pattern of `[^\\]]*` stops at the image's own `]`, so blanking
    consumed `[![alt](img)` and left `](href)` looking malformed - eleven false
    positives on this project's own README badges.
    """
    config = DocumentationConfig(enabled=True, custom_dictionary=[], markdown_checks=True)
    analyzer = DocumentationAnalyzer(config)

    md = "[![PyPI version](https://badge.fury.io/py/drep-ai.svg)](https://badge.fury.io/py/drep-ai)\n"
    findings = await analyzer.analyze_file("README.md", md)

    assert [i for i in findings.pattern_issues if i.type == "link_syntax_invalid"] == []
    # Both URLs live inside the construct, so neither is bare
    assert [i for i in findings.pattern_issues if i.type == "bare_url"] == []
