"""Unit tests for PatternLayer (Phase 3.6)."""

from drep.documentation.patterns import PatternLayer


def test_double_space_detection():
    """Test that PatternLayer detects double spaces."""
    layer = PatternLayer()
    text = "This  has  double  spaces"
    issues = layer.check(text, "md")

    assert len(issues) == 3
    assert all(i.type == "double_space" for i in issues)
    assert all(i.matched_text.startswith("  ") for i in issues)
    assert all(i.replacement == " " for i in issues)


def test_trailing_whitespace_detection():
    """Test that PatternLayer detects trailing whitespace."""
    layer = PatternLayer()
    text = "Line with trailing   \n"
    issues = layer.check(text, "md")

    assert len(issues) == 1
    assert issues[0].type == "trailing_whitespace"
    assert issues[0].matched_text == "   "
    assert issues[0].replacement == ""


def test_line_column_calculation():
    """Test that line and column numbers are calculated correctly."""
    layer = PatternLayer()
    text = "Line 1\nLine  2 with double\nLine 3"
    issues = layer.check(text, "md")

    assert len(issues) == 1
    assert issues[0].line == 2  # Second line
    assert issues[0].type == "double_space"


def test_multiple_patterns():
    """Test detecting multiple pattern types in one text."""
    layer = PatternLayer()
    text = "Line  with double space   \nAnother  line with both  \n"
    issues = layer.check(text, "md")

    # Should find 2 double spaces and 2 trailing whitespaces
    assert len(issues) == 4
    double_spaces = [i for i in issues if i.type == "double_space"]
    trailing = [i for i in issues if i.type == "trailing_whitespace"]
    assert len(double_spaces) == 2
    assert len(trailing) == 2


def test_no_issues():
    """Test that clean text returns no issues."""
    layer = PatternLayer()
    text = "This text has no pattern issues.\nJust clean prose."
    issues = layer.check(text, "md")

    assert len(issues) == 0
