//! Shared test fixtures for everything that talks to a mock LLM endpoint.
//!
//! Crate-level rather than per-module because the LLM client tests and the
//! analysis tests need exactly the same four helpers. They were duplicated
//! once - byte-identical except that the copy dropped the doc paragraph
//! explaining why `fast_retry_client` must not override `max_attempts`, which
//! is the paragraph recording a real bug. Two copies of the SSE builder in
//! particular is a trap: it encodes an SDK behaviour that fails silently.
//!
//! The SSE builder here is the one piece that cannot be guessed: the SDK
//! buffers text deltas and only emits `ContentBlock`s when a chunk carries a
//! non-null `finish_reason`. A stream where every chunk has `"finish_reason":
//! null` yields ZERO blocks, with no error and no warning, because the empty
//! result is silently dropped.

use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::LlmConfig;
use crate::llm::cache::Cache;
use crate::llm::chain::ProviderChain;
use crate::llm::client::LlmClient;

/// A config pointing at `server`, with the LLM enabled.
pub(crate) fn cfg_for(server: &MockServer, model: &str, max_retries: u32) -> LlmConfig {
    LlmConfig {
        enabled: true,
        endpoint: Some(format!("{}/v1", server.uri())),
        model: Some(model.to_owned()),
        api_key: Some("not-needed".to_owned()),
        max_retries,
        ..LlmConfig::default()
    }
}

/// Build an SSE body the SDK will parse into `parts` concatenated.
///
/// The final chunk carries `"finish_reason":"stop"`; without it the SDK emits
/// nothing at all.
pub(crate) fn sse(parts: &[&str]) -> String {
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        let finish = if i + 1 == parts.len() {
            "\"stop\""
        } else {
            "null"
        };
        out.push_str(&format!(
            "data: {{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":{finish}}}]}}\n\n",
            serde_json::to_string(part).expect("string serializes")
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

/// Build an SSE body whose final chunk carries `finish` rather than `"stop"`.
///
/// The reason the server gives is what decides whether drep asks again, so a
/// test about that decision has to be able to set it. [`sse`] is this with
/// `"stop"`.
pub(crate) fn sse_finishing_with(parts: &[&str], finish: &str) -> String {
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        let reason = if i + 1 == parts.len() {
            format!("\"{finish}\"")
        } else {
            "null".to_owned()
        };
        out.push_str(&format!(
            "data: {{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":{reason}}}]}}\n\n",
            serde_json::to_string(part).expect("string serializes")
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

/// A server answering 200 with `parts` and a final `finish_reason` of `finish`.
pub(crate) async fn server_finishing_with(parts: &[&str], finish: &str) -> MockServer {
    let server = MockServer::start().await;
    mount_sse(
        &server,
        ResponseTemplate::new(200)
            .set_body_raw(sse_finishing_with(parts, finish), "text/event-stream"),
    )
    .await;
    server
}

/// How many requests the mock server received.
pub(crate) async fn request_count(server: &MockServer) -> usize {
    // `expect`, not `unwrap_or(0)`. Mapping an unavailable request log to zero
    // makes a broken mock server indistinguishable from one that genuinely
    // received nothing - and the tests that assert "no request was made" are
    // exactly the ones that would then pass for the wrong reason.
    server
        .received_requests()
        .await
        .expect("the mock server must be recording requests")
        .len()
}

/// Build a client through the production `LlmClient::new`, then shrink only the
/// backoff delays so the retry tests do not spend seconds asleep.
///
/// It deliberately does **not** override `max_attempts`. That value comes from
/// `cfg.max_retries` through the production path, and it is the behaviour the
/// retry tests exist to pin. An earlier version took `max_attempts` as a
/// parameter and built `LlmClient` by struct literal, bypassing
/// `LlmClient::new` entirely - forcing `max_attempts = 1` in production left
/// every retry test still passing, including the one asserting the request was
/// retried more than once.
pub(crate) fn fast_retry_client(cfg: &LlmConfig) -> LlmClient {
    let mut client = LlmClient::new(cfg).expect("client builds");
    client.retry_config.initial_delay = Duration::from_millis(10);
    client.retry_config.max_delay = Duration::from_millis(50);
    client.retry_config.jitter_factor = 0.0;
    client
}

/// Mount a 200 SSE response returning `parts`, and hand back the server.
///
/// Every mock in these suites wants the same six lines; stating them once
/// keeps the endpoint path and the content type from drifting between
/// suites.
pub(crate) async fn server_returning(parts: &[&str]) -> MockServer {
    let server = MockServer::start().await;
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(sse(parts), "text/event-stream"),
    )
    .await;
    server
}

/// Mount a response with `status` and an empty body, and hand back the server.
///
/// The counterpart to [`server_returning`], beside it rather than in each
/// suite: it was written out twice, in the chain suite and the analysis suite,
/// and the second copy had already lost the paragraph explaining what an empty
/// body means to the SDK.
pub(crate) async fn server_failing_with(status: u16) -> MockServer {
    let server = MockServer::start().await;
    mount_sse(&server, ResponseTemplate::new(status)).await;
    server
}

/// Mount an arbitrary response template at the chat-completions endpoint.
pub(crate) async fn mount_sse(server: &MockServer, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(template)
        .mount(server)
        .await;
}

/// Build a provider chain through the production `ProviderChain::new`, then
/// shrink only the backoff delays so a chain test does not spend seconds
/// asleep.
///
/// Like [`fast_retry_client`], it deliberately does **not** override
/// `max_attempts`: that value comes from each entry's `max_retries` through
/// the production path, and the failover tests depend on a provider actually
/// exhausting its retries before the chain advances.
pub(crate) fn fast_retry_chain(cfgs: &[LlmConfig]) -> ProviderChain {
    let refs: Vec<&LlmConfig> = cfgs.iter().collect();
    let mut chain = ProviderChain::new(&refs).expect("chain builds from valid configs");
    for provider in &mut chain.providers {
        provider.client.retry_config.initial_delay = Duration::from_millis(10);
        provider.client.retry_config.max_delay = Duration::from_millis(50);
        provider.client.retry_config.jitter_factor = 0.0;
    }
    chain
}

/// A cache rooted in a fresh `TempDir`.
///
/// The `TempDir` is returned so the caller keeps it alive - dropping it
/// deletes the cache mid-test. Never `Cache::default_root()`: that is the
/// developer's real cache directory, and an entry one test wrote once
/// satisfied another, so an unreachable-endpoint test exited clean.
pub(crate) fn temp_cache() -> (Cache, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let cache = Cache::new(dir.path().to_path_buf(), 30, 1024 * 1024);
    (cache, dir)
}

/// Write `contents` to `path` and mark it executable, without ever holding a
/// write descriptor for it in this process.
///
/// The obvious spelling - `fs::write` then [`make_executable`] - races on
/// Linux and fails the *exec*, not the write, with `ETXTBSY` ("Text file
/// busy"). The kernel refuses to exec a file any process has open for writing,
/// and `fork` copies the whole descriptor table: a test thread spawning a
/// subprocess during the microseconds our `fs::write` descriptor is open hands
/// that child an inherited copy, which keeps the inode busy until the child
/// reaches `exec`. Nothing in the writing test is wrong, and nothing in the
/// spawning test is wrong; they simply overlap. `O_CLOEXEC` (which Rust sets)
/// does not help, because the descriptor is only closed *at* exec, after the
/// window that matters.
///
/// So the write happens in a child process instead. This process never opens
/// the file, so no fork of it can inherit a descriptor to it, and the one
/// process that does hold it - the `sh` below - has exited before this
/// function returns. Deterministic: no retry loop, no sleep.
///
/// macOS does not enforce `ETXTBSY` this way, which is why the suite was green
/// locally while flaking on Linux - where CI and the mutants sweep run, and
/// where a spurious failure inside a mutation run records a mutant as caught
/// that nothing actually caught.
pub(crate) fn write_executable(path: &std::path::Path, contents: impl AsRef<str>) {
    #[cfg(unix)]
    {
        // Contents travel as argv rather than through a pipe: no descriptor of
        // ours to inherit, and no deadlock surface if a forked child holds the
        // write end open. Stubs are a few hundred bytes, far under ARG_MAX.
        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"printf '%s' "$2" > "$1" && chmod +x "$1""#)
            .arg("sh") // $0
            .arg(path)
            .arg(contents.as_ref())
            .status()
            .expect("the writer process must start");
        assert!(
            status.success(),
            "writing the executable {} failed: {status}",
            path.display()
        );
    }
    #[cfg(not(unix))]
    std::fs::write(path, contents.as_ref()).expect("writing the executable must succeed");
}

/// Mark a file executable on unix; a no-op elsewhere.
///
/// Crate-wide because five copies of this existed across four test modules,
/// each a `#[cfg(unix)]`/`#[cfg(not(unix))]` pair. `expect`, not `unwrap`, so
/// a failure names what went wrong rather than pointing at a line number in a
/// helper.
pub(crate) fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("the file must exist before its mode is changed")
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(path, perms).expect("setting the executable bit must succeed");
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Write a `drep.toml` under `dir` pointing at `endpoint`.
///
/// The on-disk config shape stated once. It was written out longhand in three
/// places across two test modules, so the `[llm]` → `[[llm]]` array-of-tables
/// change had to be made three times — and a missed one surfaces not as a
/// failed assertion but as an opaque `ConfigError::Parse` from a test that
/// looks unrelated.
///
/// `max_retries = 1` so a test pointed at a dead endpoint fails on the first
/// attempt rather than paying the SDK's backoff schedule.
pub(crate) fn write_drep_toml(dir: &std::path::Path, endpoint: &str) {
    let body = format!(
        r#"[[llm]]
enabled = true
endpoint = "{endpoint}"
model = "m"
api_key = "not-needed"
max_retries = 1
"#
    );
    std::fs::write(dir.join("drep.toml"), body).expect("drep.toml");
}

/// A `git` command scoped to `dir` and nothing else.
///
/// The environment scrubbing matters more than it looks. These tests run under
/// `cargo test`, but also under `cargo mutants`, and under drep's own
/// pre-commit hook - and git exports `GIT_DIR`, `GIT_WORK_TREE` and
/// `GIT_INDEX_FILE` to every hook it runs. A child `git` then inherits them and
/// operates on the *outer* repository instead of the `TempDir` the test built,
/// which surfaced as `fatal: .git/index: index file open failed: Not a
/// directory` from `git worktree add` - a relative `GIT_INDEX_FILE` resolved
/// against the wrong directory. `crate::diff::run_git` scrubs the same set for
/// the same reason.
pub(crate) fn git(dir: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(dir);
    command
}

/// Initialise a git repository under `dir`, isolated from the developer's
/// global git configuration.
///
/// Three copies of this existed across the `init` test files, already diverging
/// (one asserted the `git config` calls succeeded, two discarded the result).
///
/// `core.hooksPath` is the load-bearing one. Without neutralising it, a
/// globally-set value - which this machine has - leaks into every test, so
/// `hooks::install` reaches into the developer's real shared hooks directory
/// and takes a different code path than it would on a machine without the
/// setting. An empty value reads back as present-but-blank, which drep treats
/// as unset; the tests that actually exercise the chainer set their own.
pub(crate) fn git_init(dir: &std::path::Path) {
    let output = git(dir)
        .args(["init", "--initial-branch=main"])
        .output()
        .expect("git init must run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (key, value) in [
        ("user.email", "test@example.com"),
        ("user.name", "test"),
        ("core.hooksPath", ""),
        // A developer with commit signing on and no key available would
        // otherwise fail every test here for reasons unrelated to drep.
        ("commit.gpgsign", "false"),
    ] {
        let status = git(dir)
            .args(["config", "--local", key, value])
            .status()
            .expect("git config must run");
        assert!(status.success(), "git config {key} failed");
    }
}

/// Assert `path` is executable, by the same predicate production uses.
///
/// Not a hand-rolled mode check: `languages::runner::is_executable` is what
/// decides whether drep believes a tool or a hook will run, so a test with its
/// own copy cannot detect a change to the definition it exists to protect.
pub(crate) fn assert_executable(path: &std::path::Path) {
    assert!(
        crate::languages::runner::is_executable(path),
        "{} must be executable - git ignores a non-executable hook silently",
        path.display()
    );
}

/// Clear the executable bits on `path`; a no-op where the platform has none.
///
/// The counterpart to [`make_executable`], for the tests that need to create
/// the state git treats as "this hook does not exist". The `cfg` lives inside
/// the body rather than gating two definitions, matching the rule in
/// `languages::runner`.
pub(crate) fn clear_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("the file must exist before its mode is changed")
            .permissions();
        perms.set_mode(perms.mode() & !0o111);
        std::fs::set_permissions(path, perms).expect("clearing the executable bit must succeed");
    }
    #[cfg(not(unix))]
    let _ = path;
}
