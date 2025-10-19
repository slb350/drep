"""AST utilities for docstring analysis."""

import ast
from dataclasses import dataclass
from typing import List, Optional, Tuple


@dataclass
class FunctionInfo:
    """Information about a function extracted from AST."""

    name: str
    line_number: int
    docstring: Optional[str]
    args: List[str]  # Argument names
    returns: Optional[str]  # Return type hint if present
    is_public: bool  # Not starting with _
    complexity: int  # Line count
    decorators: List[str]  # @property, @staticmethod, etc.


@dataclass
class ClassInfo:
    """Information about a class extracted from AST."""

    name: str
    line_number: int
    docstring: Optional[str]
    methods: List[FunctionInfo]
    is_public: bool


def _collect_function_nodes(node: ast.AST) -> List[Tuple[ast.AST, ast.AST]]:
    """Collect all function and async function nodes with their parent node."""
    function_nodes: List[Tuple[ast.AST, ast.AST]] = []

    for child in ast.iter_child_nodes(node):
        if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
            function_nodes.append((child, node))
        function_nodes.extend(_collect_function_nodes(child))

    return function_nodes


def extract_functions(code: str) -> List[FunctionInfo]:
    """Extract all function definitions from Python code.

    Args:
        code: Python source code

    Returns:
        List of FunctionInfo objects

    Raises:
        SyntaxError: If code has syntax errors
    """
    tree = ast.parse(code)
    functions = []

    # Collect all functions but skip nested helpers (parent is another function)
    for node, parent in _collect_function_nodes(tree):
        if isinstance(parent, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue

        # Extract docstring
        docstring = ast.get_docstring(node)

        # Get ALL argument names (positional-only, regular, *args, kw-only, **kwargs)
        args = []
        # Positional-only args (PEP 570: def foo(a, b, /, c))
        for arg in node.args.posonlyargs:
            args.append(arg.arg)
        # Regular positional/keyword args
        for arg in node.args.args:
            args.append(arg.arg)
        # *args (varargs)
        if node.args.vararg:
            args.append(f"*{node.args.vararg.arg}")
        # Keyword-only args (after * or *args)
        for arg in node.args.kwonlyargs:
            args.append(arg.arg)
        # **kwargs (keyword arguments)
        if node.args.kwarg:
            args.append(f"**{node.args.kwarg.arg}")

        # Get return annotation
        returns = ast.unparse(node.returns) if node.returns else None

        # Check if public (not starting with _)
        is_public = not node.name.startswith("_")

        # Calculate complexity (line count)
        if node.end_lineno is not None:
            complexity = node.end_lineno - node.lineno + 1
        else:
            complexity = 1

        # Get decorators
        decorators = []
        for decorator in node.decorator_list:
            try:
                decorators.append(ast.unparse(decorator))
            except Exception:
                # Fallback for complex decorators
                if isinstance(decorator, ast.Name):
                    decorators.append(decorator.id)

        functions.append(
            FunctionInfo(
                name=node.name,
                line_number=node.lineno,
                docstring=docstring,
                args=args,
                returns=returns,
                is_public=is_public,
                complexity=complexity,
                decorators=decorators,
            )
        )

    return functions


def extract_classes(code: str) -> List[ClassInfo]:
    """Extract all class definitions from Python code.

    Args:
        code: Python source code

    Returns:
        List of ClassInfo objects

    Raises:
        SyntaxError: If code has syntax errors
    """
    tree = ast.parse(code)
    classes = []

    # Only iterate over top-level nodes (tree.body), NOT nested classes
    for node in tree.body:
        if isinstance(node, ast.ClassDef):
            # Extract docstring
            docstring = ast.get_docstring(node)

            # Check if public (not starting with _)
            is_public = not node.name.startswith("_")

            # Extract methods from this class (only direct children)
            methods = []
            for item in node.body:
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    method_docstring = ast.get_docstring(item)
                    # Get ALL method argument names (same as function extraction)
                    method_args = []
                    for arg in item.args.posonlyargs:
                        method_args.append(arg.arg)
                    for arg in item.args.args:
                        method_args.append(arg.arg)
                    if item.args.vararg:
                        method_args.append(f"*{item.args.vararg.arg}")
                    for arg in item.args.kwonlyargs:
                        method_args.append(arg.arg)
                    if item.args.kwarg:
                        method_args.append(f"**{item.args.kwarg.arg}")
                    method_returns = ast.unparse(item.returns) if item.returns else None
                    method_is_public = not item.name.startswith("_")

                    if item.end_lineno is not None:
                        method_complexity = item.end_lineno - item.lineno + 1
                    else:
                        method_complexity = 1

                    method_decorators = []
                    for decorator in item.decorator_list:
                        try:
                            method_decorators.append(ast.unparse(decorator))
                        except Exception:
                            if isinstance(decorator, ast.Name):
                                method_decorators.append(decorator.id)

                    methods.append(
                        FunctionInfo(
                            name=item.name,
                            line_number=item.lineno,
                            docstring=method_docstring,
                            args=method_args,
                            returns=method_returns,
                            is_public=method_is_public,
                            complexity=method_complexity,
                            decorators=method_decorators,
                        )
                    )

            classes.append(
                ClassInfo(
                    name=node.name,
                    line_number=node.lineno,
                    docstring=docstring,
                    methods=methods,
                    is_public=is_public,
                )
            )

    return classes
