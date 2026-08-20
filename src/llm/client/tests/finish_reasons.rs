//! Retry policy derived from the server's terminal finish reason.

use crate::llm::client::Extracted;
use crate::llm::error::LlmError;
use crate::test_support::{
    cfg_for, fast_retry_client, request_count, server_finishing_with, server_without_finish_reason,
};

#[tokio::test]
async fn a_response_cut_off_at_the_token_cap_is_not_retried() {
    let server = server_finishing_with(&["Let me start by reading the file"], "length").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("no JSON was produced");

    match err {
        LlmError::ModelStopped { finish, message } => {
            assert_eq!(finish, "length", "the server's own word, kept as a tag");
            assert!(
                message.contains("output token limit"),
                "the message must name the cause, got {message:?}"
            );
            assert!(
                message.contains("split it"),
                "and be actionable, got {message:?}"
            );
        }
        other => panic!("expected ModelStopped, got {other:?}"),
    }
    assert_eq!(request_count(&server).await, 1);
}

#[tokio::test]
async fn an_empty_response_cut_off_at_the_token_cap_is_not_retried() {
    let server = server_finishing_with(&[""], "length").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("the terminal finish reason explains the empty response");

    assert!(
        matches!(err, LlmError::ModelStopped { ref finish, .. } if finish == "length"),
        "got {err:?}"
    );
    assert_eq!(request_count(&server).await, 1);
}

#[tokio::test]
async fn a_content_filter_refusal_is_not_retried() {
    let server = server_finishing_with(&["I cannot process this"], "content_filter").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("no JSON was produced");

    match err {
        LlmError::ModelStopped { finish, message } => {
            assert_eq!(finish, "content_filter");
            assert!(!message.contains("output token limit"), "got {message:?}");
        }
        other => panic!("expected ModelStopped, got {other:?}"),
    }
    assert_eq!(request_count(&server).await, 1);
}

#[tokio::test]
async fn a_model_that_stopped_without_json_is_still_retried() {
    let server = server_finishing_with(&["I am afraid I cannot help"], "stop").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");

    assert!(matches!(err, LlmError::Unparseable(_)), "got {err:?}");
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize
    );
}

#[tokio::test]
async fn a_response_with_no_finish_reason_is_retried() {
    let server = server_without_finish_reason(&["still not JSON"]).await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let err = client
        .complete_json("sys", "content")
        .await
        .expect_err("prose never parses");
    assert!(matches!(err, LlmError::Unparseable(_)), "got {err:?}");
    assert_eq!(
        request_count(&server).await,
        crate::llm::client::NO_JSON_ATTEMPTS as usize
    );
}

#[tokio::test]
async fn a_capped_response_that_still_produced_json_is_accepted() {
    let server = server_finishing_with(&[r#"{"issues": []}"#], "length").await;
    let client = fast_retry_client(&cfg_for(&server, "m", 1));

    let extracted = client
        .complete_json("sys", "content")
        .await
        .expect("the JSON parsed, so the cap is irrelevant");
    assert!(matches!(extracted, Extracted::Complete(_)));
    assert_eq!(request_count(&server).await, 1);
}
