//! One definition of the LLM response shape shared by prompt, parser and CLI adapters.

use std::sync::OnceLock;

use crate::analysis::findings::LlmSeverity;

pub(crate) const ISSUES: &str = "issues";
pub(crate) const SUMMARY: &str = "summary";
pub(crate) const LINE: &str = "line";
pub(crate) const SEVERITY: &str = "severity";
pub(crate) const CATEGORY: &str = "category";
pub(crate) const MESSAGE: &str = "message";
pub(crate) const SUGGESTION: &str = "suggestion";
pub(crate) const CODE_SNIPPET: &str = "code_snippet";
pub(crate) const COMPILE_FAILURE: &str = "compile_failure";

/// Strict schema for the response shape validated by the analyzer.
pub(crate) fn output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": [ISSUES, SUMMARY],
        "properties": {
            ISSUES: {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [LINE, SEVERITY, CATEGORY, MESSAGE, SUGGESTION, CODE_SNIPPET, COMPILE_FAILURE],
                    "properties": {
                        LINE: {"type": "integer", "minimum": 1, "maximum": u32::MAX},
                        SEVERITY: {
                            "type": "string",
                            "enum": LlmSeverity::NAMES
                        },
                        CATEGORY: {"type": "string"},
                        MESSAGE: {"type": "string"},
                        SUGGESTION: {"type": "string"},
                        CODE_SNIPPET: {"type": "string"},
                        COMPILE_FAILURE: {"type": "boolean"}
                    }
                }
            },
            SUMMARY: {"type": "string"}
        }
    })
}

/// Encoded immutable schema shared by every structured-output backend request.
pub(crate) fn output_schema_bytes() -> &'static [u8] {
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::to_vec(&output_schema())
            .expect("the in-memory response schema is always serializable")
    })
}
