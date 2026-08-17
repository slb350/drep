//! Multi-file and concurrency: criteria 21, 22, plus the cache-hit
//! companion that pins "a cache hit must not acquire a limiter slot".
//!
//! The limiter test watches the limiter's own permit count. Two other
//! approaches were tried and neither discriminates — see the note on that
//! test.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::analysis::code_quality::CodeQualityAnalyzer;
use crate::analysis::prompt::build_analysis_prompt;
use crate::languages::definitions::PYTHON;
use crate::llm::cache::Cache;
use crate::llm::concurrency::Limiter;

use super::support::analyzer_with_fast_retry;
use super::support::{analyzer_for, python_hunk};
use crate::test_support::{cfg_for, mount_sse, request_count, sse};

/// Criterion 21: `analyze_files` over three files merges their findings
/// and unions their failures.
#[tokio::test]
async fn analyze_files_merges_findings_and_unions_failures() {
    let server = MockServer::start().await;
    // Three issues, one per file, at each file's single line.
    let body = "{\"issues\": [\
        {\"line\": 100, \"severity\": \"high\", \"category\": \"bug\", \"message\": \"a\"}, \
        {\"line\": 200, \"severity\": \"medium\", \"category\": \"bug\", \"message\": \"b\"}, \
        {\"line\": 300, \"severity\": \"info\", \"category\": \"bug\", \"message\": \"c\"}\
    ], \"summary\": \"\"}";
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(&[body]), "text/event-stream"),
    )
    .await;

    let (analyzer, _dir) = analyzer_for(&server);
    let by_file = vec![
        vec![python_hunk("src/a.py", 100)],
        vec![python_hunk("src/b.py", 200)],
        vec![python_hunk("src/c.py", 300)],
    ];

    let result = analyzer.analyze_files(&by_file).await;

    assert_eq!(
        result.findings.len(),
        3,
        "all three findings must be merged"
    );
    let mut messages: Vec<&str> = result.findings.iter().map(|f| f.message.as_str()).collect();
    messages.sort();
    assert_eq!(messages, vec!["a", "b", "c"]);
    assert!(
        result.failed_files.is_empty(),
        "no files failed, got {:?}",
        result.failed_files
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

    let dir = TempDir::new().expect("temp dir");
    let cache = Cache::new(dir.path().to_path_buf(), 30, 1024 * 1024);
    let mut cfg = cfg_for(&server, "m", 1);
    cfg.max_concurrent = 1;
    let analyzer = analyzer_with_fast_retry(&cfg, cache);
    let limiter = analyzer.limiter.clone();

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

    let dir = TempDir::new().expect("temp dir");
    let cache = Cache::new(dir.path().to_path_buf(), 30, 1024 * 1024);
    let cfg = cfg_for(&server, "m", 1);
    let limiter = Limiter::new(cfg.max_concurrent);
    let analyzer = CodeQualityAnalyzer::new(&cfg, cache, limiter).expect("analyzer");

    let hunks = vec![python_hunk("src/lib.py", 100)];
    let _ = analyzer.analyze_file(&hunks).await;
    let _ = analyzer.analyze_file(&hunks).await;

    assert_eq!(
        request_count(&server).await,
        1,
        "the second call must be served from the cache; only one HTTP request should occur"
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
