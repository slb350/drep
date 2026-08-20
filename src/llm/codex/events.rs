//! Parse the owned subset of the Codex CLI's JSONL event protocol.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};

use serde::Deserialize;
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
        let header: EventHeader<'_> = decode_line(&line, line_number)?;
        let kind = header
            .kind
            .as_deref()
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
                let event: ItemEvent<'_> = decode_line(&line, line_number)?;
                let item = event.item;
                let item_kind = item
                    .as_ref()
                    .and_then(|item| item.kind.as_deref())
                    .unwrap_or("(missing)");
                match item_kind {
                    "reasoning" | "todo_list" => {}
                    "agent_message" if kind != "item.completed" => {}
                    "agent_message" => {
                        if final_message.is_some() {
                            return Err(EventError::DuplicateFinalMessage);
                        }
                        let text = item
                            .and_then(|item| item.text)
                            .ok_or_else(|| EventError::MalformedFinal("missing text".to_owned()))?;
                        final_message = Some(text.into_owned());
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
                let event: ErrorEvent<'_> = decode_line(&line, line_number)?;
                let message = event
                    .message
                    .or_else(|| event.error.and_then(|error| error.message))
                    .unwrap_or(Cow::Borrowed("no diagnostic message"));
                return Err(EventError::ReportedError(excerpt(message.as_ref(), 400)));
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

fn decode_line<'a, T: Deserialize<'a>>(
    line: &'a [u8],
    line_number: usize,
) -> Result<T, EventError> {
    serde_json::from_slice(line).map_err(|err| EventError::MalformedLine {
        line: line_number,
        message: err.to_string(),
    })
}

#[derive(Deserialize)]
struct EventHeader<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ItemEvent<'a> {
    #[serde(borrow)]
    item: Option<Item<'a>>,
}

#[derive(Deserialize)]
struct Item<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ErrorEvent<'a> {
    #[serde(borrow)]
    message: Option<Cow<'a, str>>,
    #[serde(borrow)]
    error: Option<ErrorDetail<'a>>,
}

#[derive(Deserialize)]
struct ErrorDetail<'a> {
    #[serde(borrow)]
    message: Option<Cow<'a, str>>,
}
