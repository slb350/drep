//! JSONL parsing accepts one completed structured message and no tool activity.

use serde_json::json;
use std::io::Read;

use crate::llm::codex::events::{EventError, parse_jsonl};

const CLEAN_FINAL_MESSAGE: &str = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}\n";
const TURN_COMPLETED: &str = "{\"type\":\"turn.completed\"}\n";

fn clean_lifecycle(prefix: &str) -> String {
    format!("{prefix}{CLEAN_FINAL_MESSAGE}{TURN_COMPLETED}")
}

#[test]
fn progress_then_final_message_then_turn_completion_is_success() {
    let output = concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"redacted\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"0\",\"type\":\"reasoning\",\"text\":\"hidden\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"1\",\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":3}}\n",
    );

    assert_eq!(
        parse_jsonl(output.as_bytes()).expect("valid lifecycle"),
        json!({"issues": [], "summary": "clean"})
    );
}

#[test]
fn a_tool_event_fails_closed_even_when_a_final_answer_follows() {
    let output = concat!(
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"pwd\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}\n",
        "{\"type\":\"turn.completed\"}\n",
    );

    assert!(matches!(
        parse_jsonl(output.as_bytes()),
        Err(EventError::ForbiddenItem(ref kind)) if kind == "command_execution"
    ));
}

#[test]
fn a_final_message_without_turn_completion_is_rejected() {
    let output = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"issues\\\":[],\\\"summary\\\":\\\"clean\\\"}\"}}\n";
    assert!(matches!(
        parse_jsonl(output.as_bytes()),
        Err(EventError::MissingTurnCompletion)
    ));
}

#[test]
fn fragmented_reads_do_not_change_jsonl_framing() {
    let output = clean_lifecycle("");
    let reader = Fragmented {
        bytes: output.as_bytes(),
        chunk: 3,
    };
    assert_eq!(
        parse_jsonl(reader).expect("fragmented stream"),
        json!({"issues": [], "summary": "clean"})
    );
}

#[test]
fn every_external_activity_item_is_forbidden() {
    for kind in [
        "command_execution",
        "file_change",
        "mcp_tool_call",
        "web_search",
        "subagent_call",
        "error",
    ] {
        let output = format!("{{\"type\":\"item.started\",\"item\":{{\"type\":\"{kind}\"}}}}\n");
        assert!(matches!(
            parse_jsonl(output.as_bytes()),
            Err(EventError::ForbiddenItem(ref found)) if found == kind
        ));
    }
}

#[test]
fn a_started_agent_message_is_progress_not_the_final_answer() {
    let output = clean_lifecycle(
        "{\"type\":\"item.started\",\"item\":{\"type\":\"agent_message\",\"text\":\"not final\"}}\n",
    );

    assert_eq!(
        parse_jsonl(output.as_bytes()).expect("started message is ignored"),
        json!({"issues": [], "summary": "clean"})
    );
}

#[test]
fn todo_updates_are_harmless_progress_but_unknown_events_fail_closed() {
    let output = clean_lifecycle(
        "{\"type\":\"item.updated\",\"item\":{\"type\":\"todo_list\",\"items\":[]}}\n",
    );
    assert!(parse_jsonl(output.as_bytes()).is_ok());
    assert!(matches!(
        parse_jsonl(b"{\"type\":\"future.event\"}\n".as_slice()),
        Err(EventError::UnknownEvent(ref kind)) if kind == "future.event"
    ));
}

#[test]
fn error_events_retain_only_a_bounded_sanitized_message() {
    let output = b"{\"type\":\"error\",\"message\":\"failed\\u001b[31m details\"}\n";
    let err = parse_jsonl(output.as_slice()).expect_err("terminal error");
    match err {
        EventError::ReportedError(message) => {
            assert!(message.contains("failed"), "got {message}");
            assert!(!message.chars().any(char::is_control), "got {message:?}");
        }
        other => panic!("unexpected event error: {other:?}"),
    }
}

#[test]
fn duplicate_or_post_completion_events_are_rejected() {
    let duplicate = format!("{CLEAN_FINAL_MESSAGE}{CLEAN_FINAL_MESSAGE}");
    assert!(matches!(
        parse_jsonl(duplicate.as_bytes()),
        Err(EventError::DuplicateFinalMessage)
    ));

    let post_completion =
        format!("{CLEAN_FINAL_MESSAGE}{TURN_COMPLETED}{{\"type\":\"thread.started\"}}\n");
    assert!(matches!(
        parse_jsonl(post_completion.as_bytes()),
        Err(EventError::EventAfterCompletion)
    ));
}

#[test]
fn oversized_or_malformed_lines_are_rejected_before_lifecycle_checks() {
    let oversized = vec![b'x'; 1024 * 1024 + 1];
    assert!(matches!(
        parse_jsonl(oversized.as_slice()),
        Err(EventError::LineTooLarge { .. })
    ));
    assert!(matches!(
        parse_jsonl(b"not-json\n".as_slice()),
        Err(EventError::MalformedLine { line: 1, .. })
    ));
}

#[test]
fn an_event_line_exactly_one_mebibyte_is_accepted() {
    const EXPECTED_LIMIT: usize = 1024 * 1024;
    let mut event = json!({"type": "thread.started", "padding": ""});
    let fixed = serde_json::to_vec(&event).expect("event serializes");
    event["padding"] = serde_json::Value::String("x".repeat(EXPECTED_LIMIT - fixed.len()));
    let mut output = serde_json::to_vec(&event).expect("padded event serializes");
    assert_eq!(output.len(), EXPECTED_LIMIT);
    output.push(b'\n');
    output.extend_from_slice(CLEAN_FINAL_MESSAGE.as_bytes());
    output.extend_from_slice(TURN_COMPLETED.as_bytes());

    assert_eq!(
        parse_jsonl(output.as_slice()).expect("the exact event-line ceiling is accepted"),
        json!({"issues": [], "summary": "clean"})
    );
}

struct Fragmented<'a> {
    bytes: &'a [u8],
    chunk: usize,
}

impl Read for Fragmented<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.chunk.min(buffer.len()).min(self.bytes.len());
        buffer[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}
