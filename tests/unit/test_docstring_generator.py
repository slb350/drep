"""Tests for DocstringGenerator."""

from unittest.mock import AsyncMock, MagicMock

import pytest

from drep.docstring.ast_utils import FunctionInfo
from drep.docstring.generator import DocstringGenerator
from drep.models.findings import Finding


@pytest.fixture
def mock_llm_client():
    """Create a mock LLM client."""
    client = MagicMock()
    client.analyze_code_json = AsyncMock()
    return client


@pytest.fixture
def docstring_generator(mock_llm_client):
    """Create DocstringGenerator with mocked LLM client."""
    return DocstringGenerator(llm_client=mock_llm_client)


class TestShouldAnalyze:
    """Tests for _should_analyze() logic."""

    def test_analyzes_public_complex_function(self, docstring_generator):
        """Test that public complex functions are analyzed."""
        func = FunctionInfo(
            name="calculate_total",
            line_number=10,
            docstring=None,
            args=["prices", "tax_rate"],
            returns="float",
            is_public=True,
            complexity=15,  # Complex enough
            decorators=[],
        )

        assert docstring_generator._should_analyze(func) is True

    def test_skips_private_function(self, docstring_generator):
        """Test that private functions are skipped."""
        func = FunctionInfo(
            name="_internal_helper",
            line_number=5,
            docstring=None,
            args=["x"],
            returns=None,
            is_public=False,
            complexity=10,
            decorators=[],
        )

        assert docstring_generator._should_analyze(func) is False

    def test_analyzes_property_even_if_private(self, docstring_generator):
        """Test that @property methods are analyzed even if private."""
        func = FunctionInfo(
            name="_value",
            line_number=20,
            docstring=None,
            args=["self"],
            returns="int",
            is_public=False,
            complexity=5,
            decorators=["@property"],
        )

        assert docstring_generator._should_analyze(func) is True

    def test_analyzes_classmethod(self, docstring_generator):
        """Test that @classmethod is analyzed."""
        func = FunctionInfo(
            name="from_dict",
            line_number=30,
            docstring=None,
            args=["cls", "data"],
            returns="MyClass",
            is_public=True,
            complexity=8,
            decorators=["@classmethod"],
        )

        assert docstring_generator._should_analyze(func) is True

    def test_skips_very_simple_functions(self, docstring_generator):
        """Test that very simple functions (< 3 lines) are skipped."""
        func = FunctionInfo(
            name="simple",
            line_number=1,
            docstring=None,
            args=[],
            returns=None,
            is_public=True,
            complexity=2,  # Too simple
            decorators=[],
        )

        assert docstring_generator._should_analyze(func) is False


class TestIsPoorDocstring:
    """Tests for _is_poor_docstring() detection."""

    def test_detects_short_docstring(self, docstring_generator):
        """Test that very short docstrings are flagged as poor."""
        assert docstring_generator._is_poor_docstring("TODO") is True
        assert docstring_generator._is_poor_docstring("foo") is True

    def test_detects_todo_placeholder(self, docstring_generator):
        """Test that TODO placeholders are detected."""
        assert docstring_generator._is_poor_docstring("TODO: write this later") is True
        assert docstring_generator._is_poor_docstring("FIXME: incomplete") is True

    def test_detects_generic_phrases(self, docstring_generator):
        """Test that generic phrases are detected."""
        assert docstring_generator._is_poor_docstring("This function does stuff") is True
        assert docstring_generator._is_poor_docstring("Helper function for processing") is True
        assert docstring_generator._is_poor_docstring("Utility function placeholder") is True

    def test_accepts_good_docstring(self, docstring_generator):
        """Test that good docstrings are not flagged."""
        good_docstring = """Calculate the total price including tax.

        Args:
            prices: List of item prices.
            tax_rate: Tax rate as a decimal.

        Returns:
            Total price with tax applied.
        """
        assert docstring_generator._is_poor_docstring(good_docstring) is False


class TestGenerateDocstring:
    """Tests for _generate_docstring() method."""

    @pytest.mark.asyncio
    async def test_generates_docstring_successfully(self, docstring_generator, mock_llm_client):
        """Test successful docstring generation."""
        # Setup function info
        func = FunctionInfo(
            name="calculate_total",
            line_number=10,
            docstring=None,
            args=["prices", "tax_rate"],
            returns="float",
            is_public=True,
            complexity=10,
            decorators=[],
        )

        # Mock LLM response
        docstring_text = (
            "Calculate total price with tax.\n\nArgs:\n    prices: List of prices.\n"
            "    tax_rate: Tax rate.\n\nReturns:\n    Total with tax."
        )
        mock_llm_client.analyze_code_json.return_value = {
            "docstring": docstring_text,
            "quality": "high",
            "reasoning": "Function performs price calculation with tax",
        }

        # Test file content
        file_content = """
def calculate_total(prices: List[float], tax_rate: float) -> float:
    subtotal = sum(prices)
    tax = subtotal * tax_rate
    return subtotal + tax
"""

        # Generate docstring
        finding = await docstring_generator._generate_docstring(
            file_path="test.py",
            func_info=func,
            full_content=file_content,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Verify finding created
        assert finding is not None
        assert isinstance(finding, Finding)
        assert finding.type == "missing-docstring"
        assert finding.severity == "info"
        assert finding.file_path == "test.py"
        assert finding.line == 10
        assert "calculate_total" in finding.message
        assert "```python" in finding.suggestion
        assert "Calculate total price" in finding.suggestion

        # Verify LLM was called with correct parameters
        mock_llm_client.analyze_code_json.assert_called_once()
        call_kwargs = mock_llm_client.analyze_code_json.call_args.kwargs
        assert call_kwargs["repo_id"] == "test/repo"
        assert call_kwargs["commit_sha"] == "abc123"

    @pytest.mark.asyncio
    async def test_handles_llm_error_gracefully(self, docstring_generator, mock_llm_client):
        """Test that LLM errors are handled gracefully."""
        func = FunctionInfo(
            name="test_func",
            line_number=5,
            docstring=None,
            args=[],
            returns=None,
            is_public=True,
            complexity=10,
            decorators=[],
        )

        # Mock LLM to raise exception
        mock_llm_client.analyze_code_json.side_effect = Exception("LLM timeout")

        file_content = "def test_func():\n    pass"

        # Should return None and not crash
        finding = await docstring_generator._generate_docstring(
            file_path="test.py",
            func_info=func,
            full_content=file_content,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        assert finding is None


class TestAnalyzeFile:
    """Tests for analyze_file() method."""

    @pytest.mark.asyncio
    async def test_analyzes_file_with_missing_docstrings(
        self, docstring_generator, mock_llm_client
    ):
        """Test analyzing file with functions missing docstrings."""
        # Python file with 2 functions: 1 missing docstring, 1 has docstring
        code = '''
def public_func(x: int) -> int:
    """This function has a docstring."""
    return x * 2

def another_func(a, b):
    result = a + b
    return result
'''

        # Mock LLM response
        add_docstring = (
            "Add two numbers.\n\nArgs:\n    a: First number.\n"
            "    b: Second number.\n\nReturns:\n    Sum of a and b."
        )
        mock_llm_client.analyze_code_json.return_value = {
            "docstring": add_docstring,
            "quality": "high",
            "reasoning": "Function performs simple addition",
        }

        # Analyze file
        findings = await docstring_generator.analyze_file(
            file_path="module.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should find 1 missing docstring (another_func)
        assert len(findings) == 1
        assert findings[0].type == "missing-docstring"
        assert "another_func" in findings[0].message

    @pytest.mark.asyncio
    async def test_detects_poor_docstrings(self, docstring_generator, mock_llm_client):
        """Test detecting poor-quality docstrings."""
        code = '''
def process_data(data):
    """TODO: write this."""
    return data.strip().lower()
'''

        # Mock LLM response for improving docstring
        clean_docstring = (
            "Clean and normalize text data.\n\nArgs:\n    data: Raw text data.\n"
            "\nReturns:\n    Lowercase text with whitespace removed."
        )
        mock_llm_client.analyze_code_json.return_value = {
            "docstring": clean_docstring,
            "quality": "high",
            "reasoning": "Function cleans text data",
        }

        findings = await docstring_generator.analyze_file(
            file_path="utils.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should detect poor docstring
        assert len(findings) == 1

    @pytest.mark.asyncio
    async def test_skips_private_functions(self, docstring_generator, mock_llm_client):
        """Test that private functions are skipped."""
        code = """
def _private_helper(x):
    return x * 2

def __internal__(y):
    return y + 1
"""

        findings = await docstring_generator.analyze_file(
            file_path="helpers.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should skip all private functions
        assert len(findings) == 0
        mock_llm_client.analyze_code_json.assert_not_called()

    @pytest.mark.asyncio
    async def test_handles_syntax_errors(self, docstring_generator, mock_llm_client):
        """Test that syntax errors are handled gracefully."""
        code = """
def broken(
    # Missing closing paren and colon
    return "error"
"""

        findings = await docstring_generator.analyze_file(
            file_path="broken.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should return empty list, not crash
        assert findings == []

    @pytest.mark.asyncio
    async def test_skips_simple_functions(self, docstring_generator, mock_llm_client):
        """Test that very simple functions are skipped."""
        code = """
def simple():
    pass

def getter(self):
    return self.value
"""

        findings = await docstring_generator.analyze_file(
            file_path="simple.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should skip simple functions
        assert len(findings) == 0

    @pytest.mark.asyncio
    async def test_back_to_back_functions_no_spillover(self, docstring_generator, mock_llm_client):
        """Test that back-to-back functions don't capture next function's code.

        Regression test for bug where function extraction captured one extra line,
        causing the LLM to see blended function definitions.
        """
        code = """
def first_function(x: int) -> int:
    result = x * 2
    result += 1
    return result

def second_function(y: int) -> int:
    value = y + 10
    value *= 2
    return value
"""

        # Track what code was sent to LLM
        captured_code = []

        async def capture_code(*args, **kwargs):
            # Extract the code from system_prompt
            prompt = kwargs.get("system_prompt", "")
            if "```python" in prompt:
                import re

                match = re.search(r"```python\n(.*?)\n```", prompt, re.DOTALL)
                if match:
                    captured_code.append(match.group(1))
            return {
                "docstring": "Test docstring",
                "quality": "high",
                "reasoning": "Test",
            }

        mock_llm_client.analyze_code_json.side_effect = capture_code

        await docstring_generator.analyze_file(
            file_path="test.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should have analyzed 2 functions
        assert len(captured_code) == 2

        # First function code should NOT contain "def second_function"
        assert (
            "def second_function" not in captured_code[0]
        ), f"First function code contains spillover: {captured_code[0]}"

        # Second function code should only contain second_function
        assert "def second_function" in captured_code[1]
        assert "def first_function" not in captured_code[1]

    @pytest.mark.asyncio
    async def test_no_blank_line_between_functions_causes_spillover(
        self, docstring_generator, mock_llm_client
    ):
        """Test that functions with NO blank line between them cause spillover bug.

        This demonstrates the actual bug: when functions are directly adjacent,
        the extraction captures the next function's def line.
        """
        code = """
def first_function(x: int) -> int:
    result = x * 2
    result += 1
    return result
def second_function(y: int) -> int:
    value = y + 10
    value *= 2
    return value
"""

        # Track what code was sent to LLM
        captured_code = []

        async def capture_code(*args, **kwargs):
            prompt = kwargs.get("system_prompt", "")
            if "```python" in prompt:
                import re

                match = re.search(r"```python\n(.*?)\n```", prompt, re.DOTALL)
                if match:
                    captured_code.append(match.group(1))
            return {
                "docstring": "Test docstring",
                "quality": "high",
                "reasoning": "Test",
            }

        mock_llm_client.analyze_code_json.side_effect = capture_code

        await docstring_generator.analyze_file(
            file_path="test.py",
            content=code,
            repo_id="test/repo",
            commit_sha="abc123",
        )

        # Should have analyzed 2 functions
        assert len(captured_code) == 2

        # THIS SHOULD FAIL: First function code WILL contain "def second_function"
        # because we extract one line too many
        assert (
            "def second_function" not in captured_code[0]
        ), f"BUG: First function code contains spillover!\nCaptured:\n{captured_code[0]}"
