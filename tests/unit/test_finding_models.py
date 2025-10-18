"""Tests for finding models."""


def test_typo_model_valid():
    """Test Typo model with valid data."""
    from drep.models.findings import Typo

    typo = Typo(
        word="teh",
        replacement="the",
        line=5,
        column=10,
        context="This is teh test",
        suggestions=["the", "tea"],
    )

    assert typo.word == "teh"
    assert typo.replacement == "the"
    assert typo.line == 5
    assert typo.column == 10
    assert typo.context == "This is teh test"
    assert typo.suggestions == ["the", "tea"]


def test_typo_model_empty_suggestions():
    """Test Typo model with empty suggestions list."""
    from drep.models.findings import Typo

    typo = Typo(
        word="teh", replacement="the", line=5, column=10, context="This is teh test", suggestions=[]
    )

    assert typo.suggestions == []


def test_typo_model_default_suggestions():
    """Test Typo model has default empty suggestions."""
    from drep.models.findings import Typo

    typo = Typo(word="teh", replacement="the", line=5, column=10, context="This is teh test")

    assert typo.suggestions == []


def test_pattern_issue_model_valid():
    """Test PatternIssue model with valid data."""
    from drep.models.findings import PatternIssue

    issue = PatternIssue(type="double_space", line=10, column=5, matched_text="  ", replacement=" ")

    assert issue.type == "double_space"
    assert issue.line == 10
    assert issue.column == 5
    assert issue.matched_text == "  "
    assert issue.replacement == " "


def test_finding_model_complete():
    """Test Finding model with all fields."""
    from drep.models.findings import Finding

    finding = Finding(
        type="typo",
        severity="info",
        file_path="test.py",
        line=5,
        column=10,
        original="teh",
        replacement="the",
        message="Typo: 'teh'",
        suggestion="Did you mean 'the'?",
    )

    assert finding.type == "typo"
    assert finding.severity == "info"
    assert finding.file_path == "test.py"
    assert finding.line == 5
    assert finding.column == 10
    assert finding.original == "teh"
    assert finding.replacement == "the"
    assert finding.message == "Typo: 'teh'"
    assert finding.suggestion == "Did you mean 'the'?"


def test_finding_model_optional_fields():
    """Test Finding model with only required fields."""
    from drep.models.findings import Finding

    finding = Finding(
        type="typo", severity="info", file_path="test.py", line=5, message="Typo found"
    )

    assert finding.column is None
    assert finding.original is None
    assert finding.replacement is None
    assert finding.suggestion is None


def test_documentation_findings_empty():
    """Test DocumentationFindings with no findings."""
    from drep.models.findings import DocumentationFindings

    findings = DocumentationFindings(file_path="test.py")

    assert findings.file_path == "test.py"
    assert findings.typos == []
    assert findings.pattern_issues == []


def test_documentation_findings_with_typos():
    """Test DocumentationFindings with typos."""
    from drep.models.findings import DocumentationFindings, Typo

    typo = Typo(word="teh", replacement="the", line=5, column=10, context="This is teh test")

    findings = DocumentationFindings(file_path="test.py", typos=[typo])

    assert len(findings.typos) == 1
    assert findings.typos[0].word == "teh"


def test_documentation_findings_with_patterns():
    """Test DocumentationFindings with pattern issues."""
    from drep.models.findings import DocumentationFindings, PatternIssue

    issue = PatternIssue(type="double_space", line=10, column=5, matched_text="  ", replacement=" ")

    findings = DocumentationFindings(file_path="test.py", pattern_issues=[issue])

    assert len(findings.pattern_issues) == 1
    assert findings.pattern_issues[0].type == "double_space"


def test_documentation_findings_to_findings_typos():
    """Test converting typos to generic Finding objects."""
    from drep.models.findings import DocumentationFindings, Typo

    typo = Typo(word="teh", replacement="the", line=5, column=10, context="This is teh test")

    doc_findings = DocumentationFindings(file_path="test.py", typos=[typo])

    generic_findings = doc_findings.to_findings()

    assert len(generic_findings) == 1
    assert generic_findings[0].type == "typo"
    assert generic_findings[0].severity == "info"
    assert generic_findings[0].file_path == "test.py"
    assert generic_findings[0].line == 5
    assert generic_findings[0].column == 10
    assert generic_findings[0].original == "teh"
    assert generic_findings[0].replacement == "the"
    assert "teh" in generic_findings[0].message
    assert "the" in generic_findings[0].suggestion


def test_documentation_findings_to_findings_patterns():
    """Test converting pattern issues to generic Finding objects."""
    from drep.models.findings import DocumentationFindings, PatternIssue

    issue = PatternIssue(type="double_space", line=10, column=5, matched_text="  ", replacement=" ")

    doc_findings = DocumentationFindings(file_path="test.py", pattern_issues=[issue])

    generic_findings = doc_findings.to_findings()

    assert len(generic_findings) == 1
    assert generic_findings[0].type == "pattern"
    assert generic_findings[0].severity == "info"
    assert generic_findings[0].file_path == "test.py"
    assert generic_findings[0].line == 10
    assert generic_findings[0].column == 5
    assert generic_findings[0].original == "  "
    assert generic_findings[0].replacement == " "
    assert "double_space" in generic_findings[0].message


def test_documentation_findings_to_findings_combined():
    """Test converting both typos and patterns together."""
    from drep.models.findings import DocumentationFindings, PatternIssue, Typo

    typo = Typo(word="teh", replacement="the", line=5, column=10, context="This is teh test")

    issue = PatternIssue(type="double_space", line=10, column=5, matched_text="  ", replacement=" ")

    doc_findings = DocumentationFindings(file_path="test.py", typos=[typo], pattern_issues=[issue])

    generic_findings = doc_findings.to_findings()

    assert len(generic_findings) == 2
    # First should be typo
    assert generic_findings[0].type == "typo"
    # Second should be pattern
    assert generic_findings[1].type == "pattern"


def test_typo_suggestions_not_shared_between_instances():
    """Test that Typo instances don't share the same suggestions list."""
    from drep.models.findings import Typo

    # Create first typo with no suggestions specified (uses default)
    typo1 = Typo(word="teh", replacement="the", line=1, column=0, context="teh test")

    # Create second typo with no suggestions specified
    typo2 = Typo(word="recieve", replacement="receive", line=2, column=0, context="recieve mail")

    # Modify first typo's suggestions
    typo1.suggestions.append("the")
    typo1.suggestions.append("tea")

    # Second typo should NOT have been affected
    assert typo2.suggestions == []
    assert typo1.suggestions == ["the", "tea"]


def test_documentation_findings_typos_not_shared():
    """Test that DocumentationFindings instances don't share typos list."""
    from drep.models.findings import DocumentationFindings

    findings1 = DocumentationFindings(file_path="test1.py")
    findings2 = DocumentationFindings(file_path="test2.py")

    # Add to first
    from drep.models.findings import Typo

    typo = Typo(word="teh", replacement="the", line=1, column=0, context="test")
    findings1.typos.append(typo)

    # Second should not be affected
    assert len(findings1.typos) == 1
    assert len(findings2.typos) == 0


def test_documentation_findings_patterns_not_shared():
    """Test that DocumentationFindings instances don't share pattern_issues list."""
    from drep.models.findings import DocumentationFindings, PatternIssue

    findings1 = DocumentationFindings(file_path="test1.py")
    findings2 = DocumentationFindings(file_path="test2.py")

    # Add to first
    issue = PatternIssue(type="double_space", line=1, column=0, matched_text="  ", replacement=" ")
    findings1.pattern_issues.append(issue)

    # Second should not be affected
    assert len(findings1.pattern_issues) == 1
    assert len(findings2.pattern_issues) == 0
