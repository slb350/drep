"""LLM-powered docstring generator."""

import logging
import textwrap
from typing import List, Optional

from drep.docstring.ast_utils import FunctionInfo, extract_functions
from drep.llm.client import LLMClient
from drep.models.docstring_findings import DocstringGenerationResult
from drep.models.findings import Finding

logger = logging.getLogger(__name__)


# System prompt for docstring generation
DOCSTRING_GENERATION_PROMPT = """You are an expert Python documentation writer.
Analyze the following function and generate a high-quality Google-style docstring.

**Requirements:**
1. Write in Google-style format (not NumPy or Sphinx)
2. Include brief description (1-2 sentences)
3. Args section: List each parameter with type and description
4. Returns section: Describe return value with type
5. Raises section: List exceptions if applicable
6. Be specific and technical, not generic
7. Use present tense ("Returns the sum" not "Will return")

**Function Information:**
Name: {function_name}
Arguments: {args}
Return type: {returns}
Decorators: {decorators}

**Function Code:**
```python
{function_code}
```

**Output Format:**
Return JSON with this exact structure:
{{
  "docstring": "Brief description.\\n\\nArgs:\\n    arg1: Description.\\n
arg2: Description.\\n\\nReturns:\\n    Return value description.\\n\\n
Raises:\\n    ValueError: When validation fails.",
  "quality": "high",
  "reasoning": "Brief explanation of what the function does"
}}

IMPORTANT: Use \\n for newlines in the docstring field.
Be specific about what the function does, not generic.
"""

# Tightened prompt (preferred) for robust JSON output
DOCSTRING_GENERATION_PROMPT_V2 = """You are an expert Python documentation writer.
Generate a precise Google-style docstring for the given function.

Requirements:
- Google-style format only (no NumPy or Sphinx)
- Start with a concise, specific summary (1–2 sentences)
- Args: list each parameter with type and clear description
- Returns: describe return value and type (omit if None)
- Raises: list exceptions if applicable (omit if none)
- Use present tense; be technical and non-generic

Function Information:
- Name: {function_name}
- Arguments: {args}
- Return type: {returns}
- Decorators: {decorators}

Function Code:
```python
{function_code}
```

Output JSON only with exactly these keys:
{{
  "docstring": "...",
  "quality": "high|medium|low",
  "reasoning": "Short explanation of the function's behavior"
}}

Notes:
- The docstring value must use \\n+ for newlines and must not include code fences or extra commentary.
- Return ONLY the JSON object.
"""


class DocstringGenerator:
    """Generates and evaluates docstrings for Python functions."""

    def __init__(self, llm_client: LLMClient):
        """Initialize docstring generator.

        Args:
            llm_client: Configured LLMClient instance
        """
        self.llm_client = llm_client

    async def analyze_file(
        self, file_path: str, content: str, repo_id: str, commit_sha: str
    ) -> List[Finding]:
        """Analyze Python file for missing/poor docstrings.

        Args:
            file_path: Path to the file
            content: File content
            repo_id: Repository identifier for rate limiting
            commit_sha: Current commit SHA for cache invalidation

        Returns:
            List of Finding objects for missing/poor docstrings
        """
        findings = []

        # Extract functions from file
        try:
            functions = extract_functions(content)
        except SyntaxError as e:
            logger.warning(f"Syntax error in {file_path}: {e}")
            return []

        # Filter to functions that need docstrings
        functions_to_analyze = [f for f in functions if self._should_analyze(f)]

        logger.debug(
            f"Analyzing {len(functions_to_analyze)}/{len(functions)} " f"functions in {file_path}"
        )

        # Analyze each function
        for func_info in functions_to_analyze:
            # Case 1: Missing docstring
            if func_info.docstring is None:
                finding = await self._generate_docstring(
                    file_path, func_info, content, repo_id, commit_sha
                )
                if finding:
                    findings.append(finding)

            # Case 2: Poor quality docstring
            elif self._is_poor_docstring(func_info.docstring):
                finding = await self._generate_docstring(
                    file_path, func_info, content, repo_id, commit_sha
                )
                if finding:
                    # Change type to indicate improvement
                    finding.type = "poor-docstring"
                    finding.message = f"Poor quality docstring for function '{func_info.name}'"
                    findings.append(finding)

        logger.info(f"Found {len(findings)} docstring issues in {file_path}")

        return findings

    def _should_analyze(self, func_info: FunctionInfo) -> bool:
        """Check if function should be analyzed for docstrings.

        Only analyze:
        - Public functions (not starting with _) AND complex (>= 3 lines)
        - Or functions with decorators like @property, @classmethod, @staticmethod

        Args:
            func_info: Function information from AST

        Returns:
            True if function should be analyzed
        """
        # Always analyze functions with special decorators
        special_decorators = [
            "@property",
            "@classmethod",
            "@staticmethod",
            "property",
            "classmethod",
            "staticmethod",
        ]
        if any(
            any(special in decorator for special in special_decorators)
            for decorator in func_info.decorators
        ):
            return True

        # Skip private functions (unless they have special decorators, already checked)
        if not func_info.is_public:
            return False

        # Skip very simple functions (< 3 lines)
        if func_info.complexity < 3:
            return False

        return True

    def _is_poor_docstring(self, docstring: str) -> bool:
        """Check if docstring is poor quality.

        Poor quality indicators:
        - Too short (< 10 chars)
        - Generic phrases like "TODO", "does stuff", "helper function"
        - No argument or return documentation for complex functions

        Args:
            docstring: The docstring text

        Returns:
            True if docstring is poor quality
        """
        docstring_lower = docstring.lower().strip()

        # Too short
        if len(docstring_lower) < 10:
            return True

        # Generic phrases
        poor_phrases = [
            "todo",
            "fixme",
            "does stuff",
            "helper function",
            "utility function",
            "placeholder",
        ]

        if any(phrase in docstring_lower for phrase in poor_phrases):
            return True

        return False

    async def _generate_docstring(
        self,
        file_path: str,
        func_info: FunctionInfo,
        full_content: str,
        repo_id: str,
        commit_sha: str,
    ) -> Optional[Finding]:
        """Generate docstring for function missing one.

        Args:
            file_path: Path to the file
            func_info: Function information
            full_content: Full file content
            repo_id: Repository ID
            commit_sha: Commit SHA

        Returns:
            Finding with suggested docstring, or None if generation fails
        """
        # Extract function code
        lines = full_content.split("\n")
        start = max(0, func_info.line_number - 1)
        # Use start + complexity to get exact function bounds (no spillover)
        end = min(len(lines), start + func_info.complexity)
        function_code = "\n".join(lines[start:end])

        # Prepare prompt
        prompt = DOCSTRING_GENERATION_PROMPT_V2.format(
            function_name=func_info.name,
            args=", ".join(func_info.args) if func_info.args else "None",
            returns=func_info.returns if func_info.returns else "None",
            decorators=", ".join(func_info.decorators) if func_info.decorators else "None",
            function_code=function_code,
        )

        try:
            # Call LLM with structured output
            result_dict = await self.llm_client.analyze_code_json(
                system_prompt=prompt,
                code="",  # Code is in prompt
                schema=DocstringGenerationResult,
                repo_id=repo_id,
                commit_sha=commit_sha,
                analyzer="docstring",
            )

            result = DocstringGenerationResult(**result_dict)

            # Create Finding
            finding = Finding(
                type="missing-docstring",
                severity="info",  # Docstrings are info-level
                file_path=file_path,
                line=func_info.line_number,
                column=None,
                original=None,
                replacement=None,  # Don't auto-replace
                message=f"Missing docstring for function '{func_info.name}'",
                suggestion=self._format_docstring_suggestion(
                    func_info.name, result.docstring, result.reasoning
                ),
            )

            logger.debug(f"Generated docstring for {func_info.name} (quality: {result.quality})")

            return finding

        except Exception as e:
            logger.error(f"Failed to generate docstring for {func_info.name}: {e}")
            return None

    def _format_docstring_suggestion(self, func_name: str, docstring: str, reasoning: str) -> str:
        """Format docstring suggestion for issue.

        Args:
            func_name: Function name
            docstring: Generated docstring
            reasoning: LLM reasoning

        Returns:
            Formatted suggestion with code block and reasoning
        """
        # Properly indent the docstring content to align with function indentation
        # Each line of the docstring should be indented by 4 spaces
        indented_docstring = textwrap.indent(docstring, "    ")

        return f"""Suggested docstring for `{func_name}()`:

```python
def {func_name}(...):
    \"\"\"
{indented_docstring}
    \"\"\"
```

**Reasoning:** {reasoning}
"""
