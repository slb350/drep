"""JSON extraction from LLM responses.

Five fallback strategies for turning noisy LLM output into validated dicts:

1. Extract from markdown code fences
2. Direct JSON parse
3. Fix common errors (trailing commas, single quotes)
4. Recover truncated JSON (add missing brackets)
5. Fuzzy inference from the Pydantic schema (last resort)
"""

import contextlib
import json
import logging
import re
from typing import Any

from pydantic import BaseModel

logger = logging.getLogger(__name__)


def extract_json(
    content: str, schema: type[BaseModel] | None, allow_fuzzy: bool = False
) -> dict[str, Any] | None:
    """Run the extraction strategies against one response.

    Args:
        content: Raw LLM response text
        schema: Optional Pydantic model for validation and fuzzy inference
        allow_fuzzy: Enable the last-resort schema-based fuzzy inference

    Returns:
        Parsed (and schema-validated) dict, or None if every strategy fails
    """
    # Strategy 1: Extract from markdown fences
    if "```json" in content or "```" in content:
        match = re.search(r"```(?:json)?\n(.*?)\n```", content, re.DOTALL)
        if match:
            content = match.group(1).strip()

    # Strategy 2: Direct parse
    try:
        result = json.loads(content)
        if schema:
            # Validate with Pydantic
            validated = schema(**result)
            return validated.model_dump()
        return result
    except json.JSONDecodeError:
        pass

    # Strategy 3: Fix common errors
    try:
        # Remove trailing commas before } or ]
        cleaned = re.sub(r",(\s*[}\]])", r"\1", content)
        # Replace single quotes with double quotes (naive)
        cleaned = cleaned.replace("'", '"')
        result = json.loads(cleaned)
        if schema:
            validated = schema(**result)
            return validated.model_dump()
        return result
    except (json.JSONDecodeError, Exception):
        pass

    # Strategy 4: Recover truncated JSON
    try:
        # Count braces
        open_braces = content.count("{")
        close_braces = content.count("}")
        open_brackets = content.count("[")
        close_brackets = content.count("]")

        recovered = content
        if open_braces > close_braces:
            recovered += "}" * (open_braces - close_braces)
        if open_brackets > close_brackets:
            recovered += "]" * (open_brackets - close_brackets)

        result = json.loads(recovered)
        if schema:
            validated = schema(**result)
            return validated.model_dump()
        return result
    except (json.JSONDecodeError, Exception):
        pass

    # Strategy 5: Fuzzy inference (last resort, caller opts in)
    if allow_fuzzy and schema:
        try:
            result = fuzzy_inference(content, schema)
            if result:
                return result
        except Exception as e:
            logger.debug(f"Fuzzy inference failed: {e}")

    return None


def fuzzy_inference(content: str, schema: type[BaseModel]) -> dict[str, Any] | None:
    """Attempt to extract data from malformed response using schema.

    Uses regex to extract values for expected fields.

    Args:
        content: Malformed response content
        schema: Pydantic model schema

    Returns:
        Extracted dict or None if extraction fails
    """
    # Get schema fields
    fields = schema.model_fields

    result: dict[str, Any] = {}
    for field_name, field_info in fields.items():
        # Try to extract field value using various patterns
        patterns = [
            # "field_name": "value"
            rf'"{field_name}"\s*:\s*"([^"]*)"',
            # "field_name": value (number/boolean)
            rf'"{field_name}"\s*:\s*([^,\}}\]]+)',
            # field_name: "value"
            rf"{field_name}\s*:\s*\"([^\"]*)\"",
            # Natural language: "field_name is value"
            rf'{field_name}\s+is\s+"([^"]*)"',
            # Natural language: field_name is value (number)
            rf"{field_name}\s+is\s+(\d+)",
        ]

        for pattern in patterns:
            match = re.search(pattern, content, re.IGNORECASE)
            if match:
                value = match.group(1).strip()
                # Try to convert to appropriate type
                if field_info.annotation is int:
                    with contextlib.suppress(ValueError):
                        result[field_name] = int(value)
                elif field_info.annotation is float:
                    with contextlib.suppress(ValueError):
                        result[field_name] = float(value)
                elif field_info.annotation is bool:
                    result[field_name] = value.lower() in ("true", "1", "yes")
                else:
                    result[field_name] = value
                break

    # Validate extracted data
    if result:
        try:
            validated = schema(**result)
            return validated.model_dump()
        except Exception:
            pass

    return None
