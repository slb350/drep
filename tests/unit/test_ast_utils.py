"""Tests for AST utilities for docstring analysis."""

import pytest

from drep.docstring.ast_utils import (
    extract_classes,
    extract_functions,
)


class TestExtractFunctions:
    """Tests for extract_functions()."""

    def test_extract_simple_function(self):
        """Test extracting a simple function without docstring."""
        code = """
def hello():
    return "world"
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        assert func.name == "hello"
        assert func.docstring is None
        assert func.args == []
        assert func.returns is None
        assert func.is_public is True
        assert func.decorators == []

    def test_extract_function_with_docstring(self):
        """Test extracting function with existing docstring."""
        code = '''
def greet(name):
    """Greet someone by name."""
    return f"Hello, {name}!"
'''
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        assert func.name == "greet"
        assert func.docstring == "Greet someone by name."
        assert func.args == ["name"]

    def test_extract_function_with_type_hints(self):
        """Test extracting function with type hints."""
        code = """
def calculate(x: int, y: float) -> float:
    return x + y
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        assert func.name == "calculate"
        assert func.args == ["x", "y"]
        assert func.returns == "float"

    def test_extract_function_with_decorators(self):
        """Test extracting function with decorators."""
        code = """
@property
def value(self):
    return self._value

@staticmethod
def helper():
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 2
        assert functions[0].name == "value"
        assert "@property" in functions[0].decorators or "property" in functions[0].decorators
        assert functions[1].name == "helper"
        assert (
            "@staticmethod" in functions[1].decorators or "staticmethod" in functions[1].decorators
        )

    def test_extract_includes_class_methods(self):
        """Test that class methods and instance methods are included."""
        code = """
class Service:
    def run(self):
        return True

class Manager:
    @classmethod
    def build(cls, config):
        return cls()
"""
        functions = extract_functions(code)

        assert len(functions) == 2

        # Should include both instance and class methods
        names = {func.name for func in functions}
        assert "run" in names
        assert "build" in names

    def test_extract_filters_private_functions(self):
        """Test that private functions are marked as is_public=False."""
        code = """
def public_func():
    pass

def _private_func():
    pass

def __dunder_func__():
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 3
        public = [f for f in functions if f.name == "public_func"][0]
        assert public.is_public is True

        private = [f for f in functions if f.name == "_private_func"][0]
        assert private.is_public is False

        dunder = [f for f in functions if f.name == "__dunder_func__"][0]
        assert dunder.is_public is False

    def test_extract_multiple_functions(self):
        """Test extracting multiple functions from same file."""
        code = """
def func1():
    pass

def func2(arg1, arg2):
    '''Function 2 docstring.'''
    return arg1 + arg2

def func3(x: int) -> bool:
    return x > 0
"""
        functions = extract_functions(code)

        assert len(functions) == 3
        assert [f.name for f in functions] == ["func1", "func2", "func3"]

    def test_extract_handles_syntax_error(self):
        """Test that syntax errors are handled gracefully."""
        code = """
def broken(
    # Missing closing paren
    return "error"
"""
        with pytest.raises(SyntaxError):
            extract_functions(code)

    def test_extract_calculates_complexity(self):
        """Test that complexity (line count) is calculated."""
        code = """
def simple():
    pass

def complex_function():
    x = 1
    y = 2
    z = 3
    return x + y + z
"""
        functions = extract_functions(code)

        simple = [f for f in functions if f.name == "simple"][0]
        complex_func = [f for f in functions if f.name == "complex_function"][0]

        # simple() is 2 lines, complex_function() is 5 lines
        assert simple.complexity < complex_func.complexity

    def test_extract_async_function(self):
        """Test extracting async functions."""
        code = """
async def fetch_data(url: str) -> dict:
    '''Fetch data from URL.'''
    return {}
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        assert func.name == "fetch_data"
        assert func.docstring == "Fetch data from URL."
        assert func.args == ["url"]
        assert func.returns == "dict"


class TestExtractClasses:
    """Tests for extract_classes()."""

    def test_extract_simple_class(self):
        """Test extracting a simple class."""
        code = """
class MyClass:
    '''A simple class.'''

    def method1(self):
        pass

    def method2(self, arg):
        return arg
"""
        classes = extract_classes(code)

        assert len(classes) == 1
        cls = classes[0]
        assert cls.name == "MyClass"
        assert cls.docstring == "A simple class."
        assert cls.is_public is True
        assert len(cls.methods) == 2

    def test_extract_class_without_docstring(self):
        """Test extracting class without docstring."""
        code = """
class EmptyClass:
    pass
"""
        classes = extract_classes(code)

        assert len(classes) == 1
        cls = classes[0]
        assert cls.name == "EmptyClass"
        assert cls.docstring is None

    def test_extract_private_class(self):
        """Test that private classes are marked as is_public=False."""
        code = """
class PublicClass:
    pass

class _PrivateClass:
    pass
"""
        classes = extract_classes(code)

        assert len(classes) == 2
        public = [c for c in classes if c.name == "PublicClass"][0]
        assert public.is_public is True

        private = [c for c in classes if c.name == "_PrivateClass"][0]
        assert private.is_public is False

    def test_extract_nested_class(self):
        """Test extracting nested classes."""
        code = """
class Outer:
    class Inner:
        pass
"""
        classes = extract_classes(code)

        # Should extract both Outer and Inner
        assert len(classes) >= 1
        assert any(c.name == "Outer" for c in classes)

    def test_extract_function_with_varargs(self):
        """Test extracting function with *args."""
        code = """
def process(*args):
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        # Should include *args
        assert "*args" in func.args

    def test_extract_function_with_kwargs(self):
        """Test extracting function with **kwargs."""
        code = """
def configure(**kwargs):
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        # Should include **kwargs
        assert "**kwargs" in func.args

    def test_extract_function_with_kwonly_args(self):
        """Test extracting function with keyword-only arguments."""
        code = """
def calculate(x, y, *, precision=2):
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        # Should include all args including keyword-only
        assert "x" in func.args
        assert "y" in func.args
        assert "precision" in func.args

    def test_extract_function_with_posonly_args(self):
        """Test extracting function with positional-only arguments (PEP 570)."""
        code = """
def foo(a, b, /, c, d):
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        # Should include positional-only args
        assert "a" in func.args
        assert "b" in func.args
        assert "c" in func.args
        assert "d" in func.args

    def test_extract_function_with_all_arg_types(self):
        """Test extracting function with all argument types combined."""
        code = """
def complex_sig(a, b, /, c, d, *args, e, f=10, **kwargs):
    pass
"""
        functions = extract_functions(code)

        assert len(functions) == 1
        func = functions[0]
        # Should include ALL argument types
        expected_args = ["a", "b", "c", "d", "*args", "e", "f", "**kwargs"]
        for arg in expected_args:
            assert arg in func.args, f"Missing {arg} from {func.args}"

    def test_extract_skips_nested_functions(self):
        """Test that nested functions are NOT extracted (only top-level)."""
        code = """
def outer_function():
    '''Outer function docstring.'''
    def inner_helper():
        '''Should NOT be extracted.'''
        pass
    return inner_helper()

def another_top_level():
    '''This should be extracted.'''
    pass
"""
        functions = extract_functions(code)

        # Should only extract top-level functions, NOT nested ones
        assert len(functions) == 2
        names = [f.name for f in functions]
        assert "outer_function" in names
        assert "another_top_level" in names
        # Nested function should NOT be in the list
        assert "inner_helper" not in names
