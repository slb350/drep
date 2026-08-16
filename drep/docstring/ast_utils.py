"""AST utilities for docstring analysis."""

import ast
from dataclasses import dataclass

# A node that defines a function (sync or async). Both expose .args, .name,
# .returns, .lineno, .end_lineno and .decorator_list.
FuncDefNode = ast.FunctionDef | ast.AsyncFunctionDef


@dataclass
class FunctionInfo:
    """Information about a function extracted from AST."""

    name: str
    line_number: int
    docstring: str | None
    args: list[str]  # Argument names
    returns: str | None  # Return type hint if present
    is_public: bool  # Not starting with _
    complexity: int  # Line count
    decorators: list[str]  # @property, @staticmethod, etc.


@dataclass
class ClassInfo:
    """Information about a class extracted from AST."""

    name: str
    line_number: int
    docstring: str | None
    methods: list[FunctionInfo]
    is_public: bool


def _collect_function_nodes(node: ast.AST, inside_function: bool = False) -> list[FuncDefNode]:
    """Collect function/async function nodes not nested inside another function.

    A function defined under control flow (if/try/with) *inside* a function is
    still a nested helper and is excluded; the same construct at module or
    class level is included.
    """
    function_nodes: list[FuncDefNode] = []

    for child in ast.iter_child_nodes(node):
        if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if not inside_function:
                function_nodes.append(child)
            # Descend into the function body: anything defined within is nested
            function_nodes.extend(_collect_function_nodes(child, inside_function=True))
        else:
            function_nodes.extend(_collect_function_nodes(child, inside_function))

    return function_nodes


def _extract_args(args: ast.arguments) -> list[str]:
    """Get ALL argument names (positional-only, regular, *args, kw-only, **kwargs)."""
    names = [arg.arg for arg in args.posonlyargs]  # Positional-only args (PEP 570)
    names.extend(arg.arg for arg in args.args)  # Regular positional/keyword args
    if args.vararg:
        names.append(f"*{args.vararg.arg}")
    names.extend(arg.arg for arg in args.kwonlyargs)  # Keyword-only args (after * or *args)
    if args.kwarg:
        names.append(f"**{args.kwarg.arg}")
    return names


def _unparse_decorator(decorator: ast.expr) -> str | None:
    """Unparse a decorator, falling back to the bare name for complex expressions."""
    try:
        return ast.unparse(decorator)
    except Exception:
        if isinstance(decorator, ast.Name):
            return decorator.id
        return None


def _build_function_info(node: FuncDefNode) -> FunctionInfo:
    """Build a FunctionInfo from a function definition node."""
    decorators = [
        unparsed
        for decorator in node.decorator_list
        if (unparsed := _unparse_decorator(decorator)) is not None
    ]
    return FunctionInfo(
        name=node.name,
        line_number=node.lineno,
        docstring=ast.get_docstring(node),
        args=_extract_args(node.args),
        returns=ast.unparse(node.returns) if node.returns else None,
        is_public=not node.name.startswith("_"),
        complexity=node.end_lineno - node.lineno + 1 if node.end_lineno is not None else 1,
        decorators=decorators,
    )


def extract_functions(code: str) -> list[FunctionInfo]:
    """Extract all function definitions from Python code.

    Args:
        code: Python source code

    Returns:
        List of FunctionInfo objects

    Raises:
        SyntaxError: If code has syntax errors
    """
    tree = ast.parse(code)

    # Collect all functions but skip nested helpers (defined inside another function)
    return [_build_function_info(node) for node in _collect_function_nodes(tree)]


def extract_classes(code: str) -> list[ClassInfo]:
    """Extract all class definitions from Python code.

    Args:
        code: Python source code

    Returns:
        List of ClassInfo objects

    Raises:
        SyntaxError: If code has syntax errors
    """
    tree = ast.parse(code)

    # Only iterate over top-level nodes (tree.body), NOT nested classes
    return [
        ClassInfo(
            name=node.name,
            line_number=node.lineno,
            docstring=ast.get_docstring(node),
            methods=[
                _build_function_info(item)
                for item in node.body
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef))
            ],
            is_public=not node.name.startswith("_"),
        )
        for node in tree.body
        if isinstance(node, ast.ClassDef)
    ]
