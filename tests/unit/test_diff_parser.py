"""Unit tests for diff parser."""


def test_parse_simple_diff():
    """Test parsing a simple diff with one file and one hunk."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/src/module.py b/src/module.py
index abc123..def456 100644
--- a/src/module.py
+++ b/src/module.py
@@ -10,7 +10,9 @@ def calculate(x, y):
     \"\"\"Calculate sum.\"\"\"
-    return x + y
+    result = x + y
+    logger.info(f"Calculated: {result}")
+    return result
"""

    hunks = parse_diff(diff_text)

    assert len(hunks) == 1
    hunk = hunks[0]
    assert hunk.file_path == "src/module.py"
    assert hunk.old_start == 10
    assert hunk.old_count == 7
    assert hunk.new_start == 10
    assert hunk.new_count == 9
    assert len(hunk.lines) > 0


def test_parse_multiple_files():
    """Test parsing diff with multiple files."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/file1.py b/file1.py
index aaa111..bbb222 100644
--- a/file1.py
+++ b/file1.py
@@ -1,3 +1,4 @@
+import logging
 def foo():
     pass

diff --git a/file2.py b/file2.py
index ccc333..ddd444 100644
--- a/file2.py
+++ b/file2.py
@@ -5,2 +5,3 @@ def bar():
     x = 1
+    y = 2
     return x
"""

    hunks = parse_diff(diff_text)

    assert len(hunks) == 2
    assert hunks[0].file_path == "file1.py"
    assert hunks[1].file_path == "file2.py"


def test_parse_added_lines():
    """Test extracting only added lines from hunk."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -1,3 +1,5 @@
 def test():
+    print("line 1")
+    print("line 2")
     pass
"""

    hunks = parse_diff(diff_text)
    assert len(hunks) == 1

    added = hunks[0].get_added_lines()
    assert len(added) == 2
    # Check line numbers and content
    assert added[0][0] == 2  # Line number
    assert "line 1" in added[0][1]  # Content
    assert added[1][0] == 3
    assert "line 2" in added[1][1]


def test_parse_removed_lines():
    """Test extracting only removed lines from hunk."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -1,5 +1,3 @@
 def test():
-    print("old line 1")
-    print("old line 2")
     pass
"""

    hunks = parse_diff(diff_text)
    assert len(hunks) == 1

    removed = hunks[0].get_removed_lines()
    assert len(removed) == 2
    # Removed lines tracked by old line numbers
    assert removed[0][0] == 2
    assert "old line 1" in removed[0][1]
    assert removed[1][0] == 3
    assert "old line 2" in removed[1][1]


def test_parse_context_lines():
    """Test that context lines (unchanged) are preserved."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -1,5 +1,5 @@
 def test():
     x = 1
-    y = 2
+    y = 3
     return x + y
"""

    hunks = parse_diff(diff_text)
    assert len(hunks) == 1

    # Lines should include context (space-prefixed), removed (-), and added (+)
    assert len(hunks[0].lines) > 0
    # Check we have both - and + lines
    has_removed = any(line.startswith("-") for line in hunks[0].lines)
    has_added = any(line.startswith("+") for line in hunks[0].lines)
    has_context = any(line.startswith(" ") for line in hunks[0].lines)
    assert has_removed
    assert has_added
    assert has_context


def test_get_added_lines_with_numbers():
    """Test that line numbers are correctly tracked for added lines."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -10,4 +10,6 @@ def func():
     a = 1
     b = 2
+    c = 3
+    d = 4
     return a + b
"""

    hunks = parse_diff(diff_text)
    added = hunks[0].get_added_lines()

    assert len(added) == 2
    # New lines should be at positions 12 and 13 (after line 11 which is "b = 2")
    assert added[0][0] == 12
    assert "c = 3" in added[0][1]
    assert added[1][0] == 13
    assert "d = 4" in added[1][1]


def test_parse_empty_diff():
    """Test parsing empty diff returns empty list."""
    from drep.pr_review.diff_parser import parse_diff

    hunks = parse_diff("")
    assert hunks == []


def test_parse_diff_no_changes():
    """Test parsing diff header without hunks."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc123..def456 100644
--- a/test.py
+++ b/test.py
"""

    # No @@ hunk headers, so no hunks
    hunks = parse_diff(diff_text)
    assert len(hunks) == 0


def test_get_context():
    """Test get_context() returns surrounding lines."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -1,5 +1,5 @@
 def test():
     x = 1
-    y = 2
+    y = 3
     return x + y
"""

    hunks = parse_diff(diff_text)
    context = hunks[0].get_context(context_lines=3)

    # Should return a string with the hunk's lines
    assert isinstance(context, str)
    assert "def test():" in context
    assert "x = 1" in context
    assert "return x + y" in context


def test_parse_binary_file_marker():
    """Test that binary files are handled gracefully."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/image.png b/image.png
index abc123..def456 100644
Binary files a/image.png and b/image.png differ
"""

    # Binary files don't have hunks, should parse without error
    hunks = parse_diff(diff_text)
    assert len(hunks) == 0


def test_parse_renamed_file():
    """Test parsing renamed file diff."""
    from drep.pr_review.diff_parser import parse_diff

    diff_text = """diff --git a/old_name.py b/new_name.py
similarity index 100%
rename from old_name.py
rename to new_name.py
"""

    # Renamed files without changes have no hunks
    hunks = parse_diff(diff_text)
    assert len(hunks) == 0


def test_parse_hunk_header_variations():
    """Test parsing various hunk header formats."""
    from drep.pr_review.diff_parser import parse_diff

    # Hunk with function name
    diff_text = """diff --git a/test.py b/test.py
index abc..def 100644
--- a/test.py
+++ b/test.py
@@ -42,7 +42,8 @@ def my_function():
     x = 1
+    y = 2
     return x
"""

    hunks = parse_diff(diff_text)
    assert len(hunks) == 1
    assert hunks[0].old_start == 42
    assert hunks[0].new_start == 42


def test_hunk_dataclass_fields():
    """Test DiffHunk dataclass has expected fields."""
    from drep.pr_review.diff_parser import DiffHunk

    hunk = DiffHunk(
        file_path="test.py",
        old_start=10,
        old_count=5,
        new_start=10,
        new_count=6,
        lines=[" context", "-removed", "+added"],
    )

    assert hunk.file_path == "test.py"
    assert hunk.old_start == 10
    assert hunk.old_count == 5
    assert hunk.new_start == 10
    assert hunk.new_count == 6
    assert len(hunk.lines) == 3
