//! Multi-file and concurrency: criteria 21, 22, plus the cache-hit
//! companion that pins "a cache hit must not acquire a limiter slot".
//!
//! The limiter test watches the limiter's own permit count. Two other
//! approaches were tried and neither discriminates — see the note on that
//! test.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::analysis::prompt::build_analysis_prompt;
use crate::languages::definitions::PYTHON;
use crate::llm::chain::ProviderChain;

use super::support::analyzer_with_fast_retry;
use super::support::{analyzer_for, python_hunk};
use crate::test_support::{cfg_for, request_count, sse, temp_cache};

/// Criterion 21: `analyze_files` merges findings across files and **unions**
/// their failures.
///
/// Two of the three files fail and one succeeds. All-three-fail would not
/// discriminate: `failed_files.len() == 3` over three analyzed files is also
/// what "insert every path we looked at" produces. Only a run where some file
/// succeeds can tell a union from a blanket insert, which is why the mock is
/// matched per file rather than shared.
#[tokio::test]
async fn analyze_files_merges_findings_and_unions_failures() {
    let server = MockServer::start().await;

    // `a.py` and `b.py` get a valid finding plus a record with an unknown
    // severity, which makes the file unanalyzed. `c.py` gets a clean finding.
    // Matched on the payload, which names the file it is about.
    for (file, line, failing) in [
        ("a.py", 100, true),
        ("b.py", 200, true),
        ("c.py", 300, false),
    ] {
        let bad = if failing {
            format!(
                ", {{\"line\": {line}, \"severity\": \"blocker\", \"category\": \"bug\", \"message\": \"d\"}}"
            )
        } else {
            String::new()
        };
        let body = format!(
            "{{\"issues\": [{{\"line\": {line}, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"{file}\"}}{bad}], \"summary\": \"\"}}"
        );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains(format!("src/{file}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(sse(&[&body]), "text/event-stream"),
            )
            .mount(&server)
            .await;
    }

    let (analyzer, _dir) = analyzer_for(&server);
    let by_file = vec![
        vec![python_hunk("src/a.py", 100)],
        vec![python_hunk("src/b.py", 200)],
        vec![python_hunk("src/c.py", 300)],
    ];

    let result = analyzer.analyze_files(&by_file).await;

    let mut messages: Vec<&str> = result.findings.iter().map(|f| f.message.as_str()).collect();
    messages.sort_unstable();
    assert_eq!(
        messages,
        vec!["a.py", "b.py", "c.py"],
        "every file's finding must reach the merged result"
    );

    // Sorted, though `failed_files` is a `BTreeMap` and already yields sorted
    // keys. The assertion is about *which* files failed, not about the map's
    // iteration order, and sorting keeps it that way if the container ever
    // changes underneath it.
    let mut failed: Vec<PathBuf> = result.failed_files.keys().cloned().collect();
    failed.sort();
    assert_eq!(
        failed,
        vec![PathBuf::from("src/a.py"), PathBuf::from("src/b.py")],
        "exactly the two failing files, by identity - a blanket insert would \
         also name src/c.py"
    );
}

/// Criterion 22: `analyze_file` takes a limiter slot for the LLM call.
///
/// Observed on the limiter itself, not through the mock. Two approaches were
/// tried first and neither discriminates, which is worth recording because
/// both look convincing:
///
/// - **An in-flight counter in `respond_with`.** wiremock runs that closure
///   while holding its own state lock, so requests serialise inside it no
///   matter what the analyzer does; the counter reads 1 even with the
///   acquisition deleted outright.
/// - **Wall-clock with `set_delay`.** Measured: four requests took 652 ms at
///   `max_concurrent = 1` and 595 ms at `max_concurrent = 8`. wiremock does
///   not overlap requests at all, so elapsed time says nothing about the
///   limiter.
///
/// What does discriminate is watching `available()` while the analysis runs:
/// with one permit it must reach zero, and if the acquisition is removed it
/// never leaves its maximum.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn analyze_file_holds_a_limiter_slot_for_the_llm_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(
                    sse(&["{\"issues\": [], \"summary\": \"ok\"}"]),
                    "text/event-stream",
                )
                // Widens the window the sampler has to observe the permit
                // being held. Nothing here depends on requests overlapping.
                .set_delay(std::time::Duration::from_millis(40)),
        )
        .mount(&server)
        .await;

    let (cache, _dir) = temp_cache();
    let mut cfg = cfg_for(&server, "m", 1);
    cfg.max_concurrent = 1;
    let analyzer = analyzer_with_fast_retry(&cfg, cache);
    // The limiter lives on the provider, not the analyzer: it is the budget
    // for one endpoint, and a chain of two would have two of them.
    let limiter = analyzer.chain().providers()[0].limiter().clone();

    // Four distinct files, so no two share a cache key and each one issues a
    // request that must take the permit.
    let by_file = vec![
        vec![python_hunk("src/a.py", 100)],
        vec![python_hunk("src/b.py", 200)],
        vec![python_hunk("src/c.py", 300)],
        vec![python_hunk("src/d.py", 400)],
    ];

    assert_eq!(limiter.available(), 1, "one permit before the run starts");

    let done = Arc::new(AtomicBool::new(false));
    let done_for_sampler = Arc::clone(&done);

    let analysis = async {
        let result = analyzer.analyze_files(&by_file).await;
        done.store(true, Ordering::SeqCst);
        result
    };
    let sampler = async {
        let mut lowest = usize::MAX;
        while !done_for_sampler.load(Ordering::SeqCst) {
            lowest = lowest.min(limiter.available());
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        lowest
    };

    let (result, lowest_available) = tokio::join!(analysis, sampler);

    assert!(result.findings.is_empty());
    assert!(result.failed_files.is_empty());
    assert_eq!(
        lowest_available, 0,
        "the sole permit must be held while a request is in flight; the lowest \
         observed count was {lowest_available}, so the LLM call is not going \
         through the limiter"
    );
    assert_eq!(
        limiter.available(),
        1,
        "every permit must be returned once the run finishes"
    );
}

/// Criterion 8 (companion): a cache hit must not acquire a limiter slot.
/// With `max_concurrent = 1` and a single file, the second call uses the
/// cache and the limiter never observes a held slot.
#[tokio::test]
async fn cache_hit_does_not_acquire_a_limiter_slot() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [], \"summary\": \"ok\"}";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let (cache, _dir) = temp_cache();
    let mut cfg = cfg_for(&server, "m", 1);
    // Exactly one permit, so the test can hold it and starve anything that
    // tries to acquire.
    cfg.max_concurrent = 1;
    let chain = ProviderChain::new(&[&cfg]).expect("chain builds");
    let analyzer = CodeQualityAnalyzer::new(chain, cache);

    let hunks = vec![python_hunk("src/lib.py", 100)];
    let first = analyzer.analyze_file(&hunks).await;
    // Discarding these with `let _` would let an `Err`-shaped regression hide
    // behind the request count.
    assert!(first.failed_files.is_empty(), "first call should succeed");
    let second = analyzer.analyze_file(&hunks).await;
    assert!(second.failed_files.is_empty(), "cache hit should succeed");
    assert_eq!(
        first.findings, second.findings,
        "a cache hit must return what the live call returned"
    );

    assert_eq!(
        request_count(&server).await,
        1,
        "the second call must be served from the cache; only one HTTP request should occur"
    );
    // Now prove a cache hit takes no permit, by holding the only one.
    //
    // Sampling `available()` during the call cannot work here: the cache path
    // has no await points, so it runs to completion before a concurrent
    // sampler is ever polled and the sampler observes nothing. Asserting
    // `available() == max` *after* the call is worse - it is vacuous, because
    // `analyze_file` drops its guard on the way out whether or not it took
    // one. An earlier version of this test did exactly that while claiming to
    // pin the behaviour.
    //
    // Holding the sole permit is deterministic: if the cache path acquired, it
    // would wait here forever.
    let limiter = analyzer.chain().providers()[0].limiter().clone();
    let held = limiter.acquire().await;
    assert_eq!(limiter.available(), 0, "the test holds the only permit");
    let third = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        analyzer.analyze_file(&hunks),
    )
    .await
    .expect("a cache hit must not wait on a limiter permit");
    drop(held);

    assert!(third.failed_files.is_empty());
    assert_eq!(
        third.findings, first.findings,
        "the cached result must match the live one"
    );
    assert_eq!(
        request_count(&server).await,
        1,
        "still only one HTTP request after three calls"
    );
}

/// A bounded review reserves a round only for a fresh provider pass. Another
/// process may populate the cache between the cache-only preflight and that
/// pass, so the live API must bypass cache rather than relabel the concurrent
/// entry as this process's fresh response.
#[tokio::test]
async fn analyze_files_live_bypasses_a_cache_entry() {
    let server = MockServer::start().await;
    let body = "{\"issues\": [], \"summary\": \"ok\"}";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"))
        .expect(2)
        .mount(&server)
        .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let hunks = vec![python_hunk("src/lib.py", 100)];
    let first = analyzer.analyze_file(&hunks).await;
    assert!(first.failed_files.is_empty());

    let second = analyzer.analyze_files_live(&[hunks.as_slice()]).await;
    assert!(second.failed_files.is_empty());
    assert_eq!(
        request_count(&server).await,
        2,
        "the explicitly live pass must contact the provider despite the cache"
    );
}

/// The prompt module is used by the analyzer; the analyzer's cache key
/// is built from the prompt. This pins that the prompt fed to the LLM
/// is the one the spec calls for — a regression here would silently
/// invalidate every cache entry.
#[test]
fn analyzer_uses_build_analysis_prompt_for_the_system_message() {
    let prompt = build_analysis_prompt(&PYTHON);
    assert!(prompt.contains("expert Python"));
    assert!(prompt.contains("specific concerns"));
}
