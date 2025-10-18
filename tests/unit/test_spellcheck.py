"""Unit tests for SpellcheckLayer (Phase 3.1 - Basic Structure)."""

from drep.documentation.spellcheck import SpellcheckLayer


def test_spellcheck_layer_initialization():
    """Test that SpellcheckLayer initializes correctly."""
    layer = SpellcheckLayer()
    assert layer.spell is not None


def test_spellcheck_layer_custom_words():
    """Test that custom words are loaded into the spell checker."""
    layer = SpellcheckLayer(custom_words=["gitea", "drep"])
    # Custom words should not be flagged as typos
    # We'll verify this in later tests when check() is implemented
    assert layer.spell is not None


def test_check_returns_empty_list_initially():
    """Test that stub check() method returns empty list."""
    layer = SpellcheckLayer()
    typos = layer.check("This has a typo", file_path="test.txt")
    assert typos == []
    assert isinstance(typos, list)


# Phase 3.2: _check_line() tests


def test_check_line_finds_typo():
    """Test that _check_line finds a simple typo."""
    layer = SpellcheckLayer()
    typos = layer._check_line("This has teh typo", 1)

    assert len(typos) == 1
    assert typos[0].word == "teh"
    assert len(typos[0].suggestions) > 0  # Should have some suggestions
    assert typos[0].line == 1
    assert typos[0].context == "This has teh typo"


def test_check_line_ignores_camelcase():
    """Test that _check_line ignores camelCase identifiers."""
    layer = SpellcheckLayer()
    typos = layer._check_line("The myVariable is camelCase", 1)

    # Should not flag camelCase identifiers as typos
    assert len(typos) == 0


def test_check_line_ignores_identifiers_with_numbers():
    """Test that _check_line ignores identifiers with numbers."""
    layer = SpellcheckLayer()
    typos = layer._check_line("The variable1 and test2 are fine", 1)

    assert len(typos) == 0


def test_check_line_ignores_urls():
    """Test that _check_line ignores URLs."""
    layer = SpellcheckLayer()
    typos = layer._check_line("Check https://example.com for docs", 1)

    # Should not flag parts of URLs as typos
    assert len(typos) == 0


def test_check_line_with_typo_and_url():
    """Test that _check_line finds typos but ignores URLs."""
    layer = SpellcheckLayer()
    typos = layer._check_line("Check https://example.com for teh docs", 1)

    # Should find 'teh' but ignore the URL
    assert len(typos) == 1
    assert typos[0].word == "teh"


def test_check_line_ignores_inline_code():
    """Test that _check_line ignores content in backticks."""
    layer = SpellcheckLayer()
    typos = layer._check_line("Use `somethng` in your code", 1)

    # Should ignore misspelled word in backticks
    assert len(typos) == 0


def test_check_line_with_typo_and_inline_code():
    """Test that _check_line finds typos but ignores inline code."""
    layer = SpellcheckLayer()
    typos = layer._check_line("Use `somethng` for teh function", 1)

    # Should find 'teh' but ignore the word in backticks
    assert len(typos) == 1
    assert typos[0].word == "teh"


# Phase 3.3: _check_markdown() tests


def test_check_markdown_skips_code_blocks():
    """Test that _check_markdown skips content inside code fences."""
    layer = SpellcheckLayer()
    text = """# Title

This has teh typo in prose.

```python
# This code has teh typo but should be ignored
def test():
    pass
```

Another teh typo in prose.
"""
    typos = layer._check_markdown(text)

    # Should find 2 typos (lines 3 and 11), not the one in code block
    assert len(typos) == 2
    assert all(t.word == "teh" for t in typos)
    # Verify line numbers are correct (prose lines, not code block)
    line_numbers = [t.line for t in typos]
    assert 3 in line_numbers
    assert 11 in line_numbers


def test_check_markdown_handles_unclosed_code_blocks():
    """Test that _check_markdown handles unclosed code blocks."""
    layer = SpellcheckLayer()
    text = """Prose with teh typo.

```
Code with teh typo
"""
    typos = layer._check_markdown(text)

    # Should only find typo in prose (line 1)
    assert len(typos) == 1
    assert typos[0].line == 1


def test_check_markdown_multiple_code_blocks():
    """Test that _check_markdown handles multiple code blocks."""
    layer = SpellcheckLayer()
    text = """Typo teh here.

```
code
```

Typo teh here too.

```
more code
```

And teh final typo.
"""
    typos = layer._check_markdown(text)

    # Should find 3 typos in prose
    assert len(typos) == 3
    assert all(t.word == "teh" for t in typos)


# Phase 3.4: _check_python_comments() tests


def test_check_python_comments_finds_comment_typos():
    """Test that _check_python_comments finds typos in # comments."""
    layer = SpellcheckLayer()
    code = """
def test_function():
    # This comment has teh typo
    x = 1
    return x  # Another teh here
"""
    typos = layer._check_python_comments(code)

    # Should find 2 typos in comments
    assert len(typos) == 2
    assert all(t.word == "teh" for t in typos)


def test_check_python_comments_finds_docstring_typos():
    """Test that _check_python_comments finds typos in docstrings."""
    layer = SpellcheckLayer(custom_words=["docstring"])
    code = '''
def test_function():
    """This docstring has teh typo."""
    return 1
'''
    typos = layer._check_python_comments(code)

    # Should find 1 typo in docstring
    assert len(typos) == 1
    assert typos[0].word == "teh"


def test_check_python_comments_ignores_code():
    """Test that _check_python_comments ignores actual Python code."""
    layer = SpellcheckLayer()
    code = """
def somethng():
    x = variabl + 1
    return x
"""
    typos = layer._check_python_comments(code)

    # Should not flag code identifiers
    assert len(typos) == 0


def test_check_python_comments_handles_syntax_errors():
    """Test that _check_python_comments handles syntax errors gracefully."""
    layer = SpellcheckLayer()
    code = """
def broken(
    # This has teh typo but file has syntax error
"""
    typos = layer._check_python_comments(code)

    # Should still find comment typo despite syntax error
    assert len(typos) == 1
    assert typos[0].word == "teh"


def test_check_python_comments_combined():
    """Test _check_python_comments with both comments and docstrings."""
    layer = SpellcheckLayer(custom_words=["docstring"])
    code = '''
def my_function():
    """Docstring with teh typo."""
    # Comment with teh typo
    x = 1  # Inline teh typo
    return x

class MyClass:
    """Class docstring with teh typo."""
    pass
'''
    typos = layer._check_python_comments(code)

    # Should find 4 typos total (all "teh")
    assert len(typos) == 4
    assert all(t.word == "teh" for t in typos)


# Phase 3.5: Wire up check() method routing


def test_check_routes_to_markdown():
    """Test that check() routes .md files to _check_markdown()."""
    layer = SpellcheckLayer()
    text = """Prose with teh typo.

```
Code with teh typo
```
"""
    typos = layer.check(text, file_path="test.md")

    # Should only find typo in prose (markdown handling)
    assert len(typos) == 1
    assert typos[0].word == "teh"


def test_check_routes_to_python():
    """Test that check() routes .py files to _check_python_comments()."""
    layer = SpellcheckLayer(custom_words=["docstring"])
    code = '''
def test():
    """Docstring with teh typo."""
    x = 1  # Comment with teh typo
    return x
'''
    typos = layer.check(code, file_path="test.py")

    # Should find typos in comments and docstring only
    assert len(typos) == 2
    assert all(t.word == "teh" for t in typos)


def test_check_routes_to_plain_text():
    """Test that check() routes unknown extensions to _check_plain_text()."""
    layer = SpellcheckLayer()
    text = """Line 1 with teh typo.
Line 2 with another teh typo.
"""
    typos = layer.check(text, file_path="test.txt")

    # Should find both typos
    assert len(typos) == 2
    assert all(t.word == "teh" for t in typos)
