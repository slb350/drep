//! Parse the owned subset of the Codex CLI's JSONL event protocol.

use std::io::{BufRead, BufReader, Read};

use serde_json::Value;
use thiserror::Error;

use crate::text::excerpt;

const EVENT_LINE_MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum EventError {
    #[error("could not read Codex event stream: {0}")]
    Read(String),
    #[error("Codex event line {line} exceeds {limit} bytes")]
    LineTooLarge { line: usize, limit: usize },
    #[error("Codex event line {line} is not valid JSON: {message}")]
    MalformedLine { line: usize, message: String },
    #[error("Codex emitted unsupported event type `{0}`")]
    UnknownEvent(String),
    #[error("Codex attempted forbidden item type `{0}`")]
    ForbiddenItem(String),
    #[error("Codex emitted more than one final agent message")]
    DuplicateFinalMessage,
    #[error("Codex emitted an event after `turn.completed`")]
    EventAfterCompletion,
    #[error("Codex emitted more than one `turn.completed` event")]
    DuplicateTurnCompletion,
    #[error("Codex completed a turn before emitting its final agent message")]
    CompletionBeforeMessage,
    #[error("Codex event stream ended without a final agent message")]
    MissingFinalMessage,
    #[error("Codex event stream ended without `turn.completed`")]
    MissingTurnCompletion,
    #[error("Codex final agent message is not valid JSON: {0}")]
    MalformedFinal(String),
    #[error("Codex reported an error: {0}")]
    ReportedError(String),
}

/// Parse a complete, bounded stdout stream.
pub(crate) fn parse_jsonl(input: impl Read) -> Result<Value, EventError> {
    let mut final_message: Option<String> = None;
    let mut turn_completed = false;

    for (offset, line) in BufReader::new(input).split(b'\n').enumerate() {
        let line_number = offset + 1;
        let line = line.map_err(|err| EventError::Read(err.to_string()))?;
        if line.len() > EVENT_LINE_MAX_BYTES {
            return Err(EventError::LineTooLarge {
                line: line_number,
                limit: EVENT_LINE_MAX_BYTES,
            });
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let event: Value =
            serde_json::from_slice(&line).map_err(|err| EventError::MalformedLine {
                line: line_number,
                message: err.to_string(),
            })?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| EventError::UnknownEvent("(missing)".to_owned()))?;

        if turn_completed {
            return Err(match kind {
                "turn.completed" => EventError::DuplicateTurnCompletion,
                _ => EventError::EventAfterCompletion,
            });
        }

        match kind {
            "thread.started" | "turn.started" => {}
            "item.started" | "item.updated" | "item.completed" => {
                let item = event.get("item").unwrap_or(&Value::Null);
                let item_kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("(missing)");
                match item_kind {
                    "reasoning" | "todo_list" => {}
                    "agent_message" if kind != "item.completed" => {}
                    "agent_message" => {
                        if final_message.is_some() {
                            return Err(EventError::DuplicateFinalMessage);
                        }
                        let text = item
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(|| EventError::MalformedFinal("missing text".to_owned()))?;
                        final_message = Some(text.to_owned());
                    }
                    other => return Err(EventError::ForbiddenItem(other.to_owned())),
                }
            }
            "turn.completed" => {
                if final_message.is_none() {
                    return Err(EventError::CompletionBeforeMessage);
                }
                turn_completed = true;
            }
            "error" | "turn.failed" => {
                let message = event
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        event
                            .get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("no diagnostic message");
                return Err(EventError::ReportedError(excerpt(message, 400)));
            }
            other => return Err(EventError::UnknownEvent(other.to_owned())),
        }
    }

    let message = final_message.ok_or(EventError::MissingFinalMessage)?;
    if !turn_completed {
        return Err(EventError::MissingTurnCompletion);
    }
    serde_json::from_str(&message).map_err(|err| EventError::MalformedFinal(err.to_string()))
}
