"""Tests for LLM findings models."""

import pytest
from pydantic import ValidationError


def test_code_issue_model_valid():
    """Test CodeIssue model with valid data."""
    from drep.models.llm_findings import CodeIssue

    issue = CodeIssue(
        line=42,
        severity="high",
        category="bug",
        message="Potential null pointer dereference",
        suggestion="Add null check before dereferencing",
        code_snippet="user.name.upper()",
    )

    assert issue.line == 42
    assert issue.severity == "high"
    assert issue.category == "bug"
    assert issue.message == "Potential null pointer dereference"
    assert issue.suggestion == "Add null check before dereferencing"
    assert issue.code_snippet == "user.name.upper()"


def test_code_issue_severity_levels():
    """Test all valid severity levels for CodeIssue."""
    from drep.models.llm_findings import CodeIssue

    severities = ["critical", "high", "medium", "low", "info"]

    for severity in severities:
        issue = CodeIssue(
            line=1,
            severity=severity,
            category="test",
            message="Test message",
            suggestion="Test suggestion",
            code_snippet="test code",
        )
        assert issue.severity == severity


def test_code_issue_invalid_severity():
    """Test CodeIssue rejects invalid severity."""
    from drep.models.llm_findings import CodeIssue

    with pytest.raises(ValidationError):
        CodeIssue(
            line=1,
            severity="extreme",  # Invalid severity
            category="test",
            message="Test message",
            suggestion="Test suggestion",
            code_snippet="test code",
        )


def test_code_issue_line_number_must_be_positive():
    """Test CodeIssue requires positive line numbers."""
    from drep.models.llm_findings import CodeIssue

    with pytest.raises(ValidationError):
        CodeIssue(
            line=0,  # Line numbers start at 1
            severity="info",
            category="test",
            message="Test message",
            suggestion="Test suggestion",
            code_snippet="test code",
        )

    with pytest.raises(ValidationError):
        CodeIssue(
            line=-5,  # Negative line numbers are invalid
            severity="info",
            category="test",
            message="Test message",
            suggestion="Test suggestion",
            code_snippet="test code",
        )


def test_code_analysis_result_empty():
    """Test CodeAnalysisResult with no issues."""
    from drep.models.llm_findings import CodeAnalysisResult

    result = CodeAnalysisResult(issues=[], summary="No issues found. Code looks good!")

    assert result.issues == []
    assert result.summary == "No issues found. Code looks good!"


def test_code_analysis_result_with_issues():
    """Test CodeAnalysisResult with multiple issues."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    issues = [
        CodeIssue(
            line=10,
            severity="critical",
            category="security",
            message="SQL injection vulnerability",
            suggestion="Use parameterized queries",
            code_snippet="cursor.execute(f'SELECT * FROM users WHERE id={user_id}')",
        ),
        CodeIssue(
            line=25,
            severity="medium",
            category="best-practice",
            message="Missing docstring",
            suggestion="Add docstring describing function purpose",
            code_snippet="def process_data(data):",
        ),
    ]

    result = CodeAnalysisResult(
        issues=issues,
        summary="Found 2 issues: 1 critical security vulnerability, 1 best-practice violation",
    )

    assert len(result.issues) == 2
    assert result.issues[0].severity == "critical"
    assert result.issues[1].severity == "medium"
    assert "2 issues" in result.summary


def test_code_analysis_result_default_empty_issues():
    """Test CodeAnalysisResult defaults to empty issues list."""
    from drep.models.llm_findings import CodeAnalysisResult

    result = CodeAnalysisResult(summary="Clean code!")

    assert result.issues == []


def test_code_analysis_result_to_findings_empty():
    """Test converting empty CodeAnalysisResult to findings."""
    from drep.models.llm_findings import CodeAnalysisResult

    result = CodeAnalysisResult(issues=[], summary="No issues found")
    findings = result.to_findings(file_path="test.py")

    assert findings == []


def test_code_analysis_result_to_findings_single_issue():
    """Test converting CodeAnalysisResult with one issue to Finding."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    issue = CodeIssue(
        line=42,
        severity="high",
        category="bug",
        message="Variable may be undefined",
        suggestion="Initialize variable before use",
        code_snippet="print(result)",
    )

    result = CodeAnalysisResult(issues=[issue], summary="1 bug found")
    findings = result.to_findings(file_path="src/main.py")

    assert len(findings) == 1

    finding = findings[0]
    assert finding.type == "bug"
    assert finding.severity == "error"  # high -> error
    assert finding.file_path == "src/main.py"
    assert finding.line == 42
    assert finding.column is None
    assert finding.original == "print(result)"
    assert finding.replacement is None
    assert finding.message == "Variable may be undefined"
    assert finding.suggestion == "Initialize variable before use"


def test_code_analysis_result_to_findings_severity_mapping():
    """Test severity mapping from LLM to Finding format."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    # Test all severity mappings
    test_cases = [
        ("critical", "error"),
        ("high", "error"),
        ("medium", "warning"),
        ("low", "info"),
        ("info", "info"),
    ]

    for llm_severity, expected_finding_severity in test_cases:
        issue = CodeIssue(
            line=1,
            severity=llm_severity,
            category="test",
            message=f"Test {llm_severity}",
            suggestion="Fix it",
            code_snippet="code",
        )

        result = CodeAnalysisResult(issues=[issue], summary="Test")
        findings = result.to_findings(file_path="test.py")

        assert findings[0].severity == expected_finding_severity


def test_code_analysis_result_to_findings_multiple_issues():
    """Test converting multiple issues with different severities."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    issues = [
        CodeIssue(
            line=10,
            severity="critical",
            category="security",
            message="Security issue",
            suggestion="Fix security",
            code_snippet="bad_code()",
        ),
        CodeIssue(
            line=20,
            severity="medium",
            category="performance",
            message="Performance issue",
            suggestion="Optimize this",
            code_snippet="slow_code()",
        ),
        CodeIssue(
            line=30,
            severity="info",
            category="style",
            message="Style issue",
            suggestion="Format better",
            code_snippet="ugly_code()",
        ),
    ]

    result = CodeAnalysisResult(issues=issues, summary="Multiple issues")
    findings = result.to_findings(file_path="app.py")

    assert len(findings) == 3

    # Check critical -> error
    assert findings[0].severity == "error"
    assert findings[0].type == "security"
    assert findings[0].line == 10

    # Check medium -> warning
    assert findings[1].severity == "warning"
    assert findings[1].type == "performance"
    assert findings[1].line == 20

    # Check info -> info
    assert findings[2].severity == "info"
    assert findings[2].type == "style"
    assert findings[2].line == 30


def test_code_analysis_result_to_findings_preserves_all_fields():
    """Test that all CodeIssue fields are preserved in Finding conversion."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    issue = CodeIssue(
        line=100,
        severity="high",
        category="bug",
        message="Division by zero possible",
        suggestion="Add check for zero denominator",
        code_snippet="result = numerator / denominator",
    )

    result = CodeAnalysisResult(issues=[issue], summary="Division by zero")
    findings = result.to_findings(file_path="calculator.py")

    finding = findings[0]
    assert finding.file_path == "calculator.py"
    assert finding.line == 100
    assert finding.type == "bug"
    assert finding.message == "Division by zero possible"
    assert finding.suggestion == "Add check for zero denominator"
    assert finding.original == "result = numerator / denominator"


def test_code_analysis_result_issues_not_shared():
    """Test that CodeAnalysisResult instances don't share issues list."""
    from drep.models.llm_findings import CodeAnalysisResult, CodeIssue

    result1 = CodeAnalysisResult(summary="Result 1")
    result2 = CodeAnalysisResult(summary="Result 2")

    # Add issue to first result
    issue = CodeIssue(
        line=1,
        severity="info",
        category="test",
        message="Test",
        suggestion="Fix",
        code_snippet="code",
    )
    result1.issues.append(issue)

    # Second result should not be affected
    assert len(result1.issues) == 1
    assert len(result2.issues) == 0


def test_code_issue_categories():
    """Test various common code issue categories."""
    from drep.models.llm_findings import CodeIssue

    categories = ["bug", "security", "best-practice", "performance", "style", "maintainability"]

    for category in categories:
        issue = CodeIssue(
            line=1,
            severity="medium",
            category=category,
            message=f"{category} issue",
            suggestion=f"Fix {category}",
            code_snippet="code",
        )
        assert issue.category == category
