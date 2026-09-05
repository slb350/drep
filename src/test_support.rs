//! Shared mock endpoints, filesystem fixtures and Git helpers.
//!
//! Since open-agent-sdk 0.10.0, each SSE delta arrives as its own `StreamEvent`.
//! Transport closure emits `Finish` even without a wire `finish_reason`, yielding
//! `Unspecified`. [`sse`] explicitly finishes with `stop`; the other builders
//! exercise that distinct unspecified outcome and provider-supplied reasons.

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
/// One chunk per part, so a multi-part `parts` is what a fragmented response
/// looks like on the wire. The final chunk carries `"finish_reason":"stop"`,
/// which is what most of these tests want: the alternative is `Unspecified`,
/// a distinct answer that drep treats differently.
pub(crate) fn sse(parts: &[&str]) -> String {
    sse_chunks(parts, "\"stop\"")
}

/// Build an SSE body whose final chunk carries `finish` rather than `"stop"`.
///
/// The reason the server gives is what decides whether drep asks again, so a
/// test about that decision has to be able to set it. [`sse`] is this with
/// `"stop"`.
pub(crate) fn sse_finishing_with(parts: &[&str], finish: &str) -> String {
    sse_chunks(parts, &format!("\"{finish}\""))
}

/// Build an SSE body whose chunks all carry `"finish_reason":null`.
///
/// llama.cpp, vLLM and several local gateways stream content and then close
/// the connection without ever reporting a reason. The SDK finishes such a
/// stream as `FinishReason::Unspecified`, which is neither `Stop` nor a
/// failure - and drep has to keep those apart, since `Unspecified` says
/// nothing about whether asking again could help.
pub(crate) fn sse_without_finish_reason(parts: &[&str]) -> String {
    sse_chunks(parts, "null")
}

/// One chunk per part, `last` being the raw JSON token for the final chunk's
/// `finish_reason`. Every earlier chunk reports `null`, which is what the wire
/// format says about a stream still in progress.
fn sse_chunks(parts: &[&str], last: &str) -> String {
    assert!(
        !parts.is_empty(),
        "an SSE fixture needs a final chunk to carry its finish reason"
    );
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        let finish = if i + 1 == parts.len() { last } else { "null" };
        out.push_str(&format!(
            "data: {{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":{finish}}}]}}\n\n",
            serde_json::to_string(part).expect("string serializes")
        ));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

/// A server answering 200 with `parts` and no `finish_reason` at all.
pub(crate) async fn server_without_finish_reason(parts: &[&str]) -> MockServer {
    sse_server(sse_without_finish_reason(parts)).await
}

/// A server answering 200 with `parts` and a final `finish_reason` of `finish`.
pub(crate) async fn server_finishing_with(parts: &[&str], finish: &str) -> MockServer {
    sse_server(sse_finishing_with(parts, finish)).await
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
/// retry tests exist to pin. Constructing the client by literal or overriding
/// attempts would hide regressions in that production configuration path.
pub(crate) fn fast_retry_client(cfg: &LlmConfig) -> LlmClient {
    let mut client = LlmClient::new(cfg).expect("client builds");
    client.retry_config.initial_delay = Duration::from_millis(10);
    client.retry_config.max_delay = Duration::from_millis(50);
    client.retry_config.jitter_factor = 0.0;
    client
}

/// Mount a 200 SSE response returning `parts`, and hand back the server.
pub(crate) async fn server_returning(parts: &[&str]) -> MockServer {
    sse_server(sse(parts)).await
}

/// A server answering 200 at the chat-completions endpoint with `body` as an
/// event stream.
async fn sse_server(body: String) -> MockServer {
    let server = MockServer::start().await;
    mount_sse(
        &server,
        ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"),
    )
    .await;
    server
}

/// Mount a response with `status` and an empty body, and hand back the server.
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

/// A server answering `GET route` with `status` and `body` as JSON.
pub(crate) async fn json_server(route: &str, status: u16, body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_string(body)
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;
    server
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
        if let Some(client) = provider.backend.http_mut() {
            client.retry_config.initial_delay = Duration::from_millis(10);
            client.retry_config.max_delay = Duration::from_millis(50);
            client.retry_config.jitter_factor = 0.0;
        }
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

/// Report whether a Unix test process is still running, then stop it if so.
///
/// Grandchild-pipe tests deliberately leave a descendant alive after the
/// configured child exits. Centralizing the cleanup keeps those fixtures from
/// accumulating sleepers on a parallel or mutation-test runner.
#[cfg(unix)]
pub(crate) fn probe_and_stop_process(pid: &str) -> bool {
    std::process::Command::new("/bin/kill")
        .arg(pid)
        .status()
        .expect("stop grandchild")
        .success()
}

/// Set a fixture's age without sleeping or depending on the wall clock.
pub(crate) fn set_mtime(path: &std::path::Path, mtime: std::time::SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime set");
    file.set_modified(mtime).expect("set_modified");
}

/// Sum regular-file bytes one directory level beneath `root`.
pub(crate) fn two_level_tree_size(root: &std::path::Path) -> u64 {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|directory| std::fs::read_dir(directory.path()).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

/// Mark a file executable on unix; a no-op elsewhere.
///
/// Preserve the file's other permission bits.
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

/// Write an arbitrary machine-policy fixture beside a test repository.
pub(crate) fn write_site_policy_body(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("site.toml");
    std::fs::write(&path, body).expect("write site policy");
    path
}

/// Write a machine-policy fixture naming `markers`.
pub(crate) fn write_site_policy(dir: &std::path::Path, markers: &[&str]) -> std::path::PathBuf {
    let quoted: Vec<String> = markers.iter().map(|marker| format!("{marker:?}")).collect();
    write_site_policy_body(dir, &format!("refuse_markers = [{}]\n", quoted.join(", ")))
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
        // The object-database trio, for the same reason as the four above:
        // they redirect where a child `git` reads and writes objects, so an
        // inherited one points at the outer repository's store while every
        // other setting names the intended one.
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_QUARANTINE_PATH")
        .current_dir(dir);
    command
}

/// Initialise a git repository under `dir`, isolated from the developer's
/// global git configuration.
///
/// `core.hooksPath` is the load-bearing one. Without neutralising it, a
/// globally-set value - which this machine has - leaks into every test, so
/// `hooks::install` reaches into the developer's real shared hooks directory
/// and takes a different code path than it would on a machine without the
/// setting. An empty value reads back as present-but-blank, which drep treats
/// as unset; the tests that actually exercise the chainer set their own.
pub(crate) fn git_init(dir: &std::path::Path) {
    git_must(dir, &["init", "--initial-branch=main"]);
    for (key, value) in [
        ("user.email", "test@example.com"),
        ("user.name", "test"),
        ("core.hooksPath", ""),
        // A developer with commit signing on and no key available would
        // otherwise fail every test here for reasons unrelated to drep.
        ("commit.gpgsign", "false"),
    ] {
        git_must(dir, &["config", "--local", key, value]);
    }
}

/// Make `dir` a directory git refuses to resolve a repository root for.
///
/// A plain `tempfile::tempdir()` is *usually* outside any repository, and a test
/// that needs "no repository here" has been relying on that. It is a property of
/// the developer's machine, not of the fixture: `TMPDIR` inside a checkout makes
/// `git rev-parse --show-toplevel` answer, and a fail-closed test then passes for
/// the wrong reason. A `.git` file holding something that is not a gitfile stops
/// discovery at this directory with `fatal: invalid gitfile format`, whatever sits
/// above it - and it denies the root through git's own discovery rather than by
/// arranging the filesystem and hoping.
pub(crate) fn git_unresolvable(dir: &std::path::Path) {
    std::fs::write(dir.join(".git"), "not a gitfile\n").expect("unresolvable .git");
}

/// Stage `path` in the repository at `dir`.
///
/// A tracked file is the one state where `.gitignore` silently does nothing, so
/// reaching it in a test needs a real `git add` rather than a file on disk.
pub(crate) fn git_add(dir: &std::path::Path, path: &str) {
    git_must(dir, &["add", "--", path]);
}

/// Stage every path and create one unsigned, hook-free fixture commit.
pub(crate) fn git_commit_all(dir: &std::path::Path, message: &str) {
    git_must(dir, &["add", "--all"]);
    git_must(dir, &["commit", "--quiet", "--no-verify", "-m", message]);
}

/// Run a Git fixture command that must succeed and return trimmed UTF-8 output.
pub(crate) fn git_output(dir: &std::path::Path, args: &[&str]) -> String {
    let output = git(dir)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("git {} must run: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is utf8")
        .trim()
        .to_owned()
}

/// Run a git command that must succeed, failing the test with its stderr.
fn git_must(dir: &std::path::Path, args: &[&str]) {
    git_output(dir, args);
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

/// A models.dev-shaped document, cut down to the cases that matter.
///
/// Field-for-field the shape of the real one, verified against
/// `https://models.dev/api.json`: providers keyed by vendor id, each with an
/// `api` URL that may be null, and models keyed by id carrying `temperature`
/// and `limit.output` among many fields drep ignores.
///
/// The registry and wizard tests share it. `glm-5.3` refuses temperature and
/// `glm-5.2` accepts it, so both outcomes are reachable from the same endpoint.
pub(crate) const MODELS_DEV_DOCUMENT: &str = r#"{
  "kimi-for-coding": {
    "id": "kimi-for-coding",
    "name": "Kimi For Coding",
    "api": "https://api.kimi.com/coding/v1",
    "models": {
      "k3": {
        "id": "k3",
        "name": "Kimi K3",
        "reasoning": true,
        "temperature": false,
        "limit": { "context": 262144, "output": 131072 }
      },
      "kimi-for-coding": {
        "id": "kimi-for-coding",
        "temperature": false,
        "limit": { "context": 262144, "output": 32768 }
      }
    }
  },
  "zai-coding-plan": {
    "id": "zai-coding-plan",
    "api": "https://api.z.ai/api/coding/paas/v4",
    "models": {
      "glm-5.3": {
        "id": "glm-5.3",
        "temperature": false,
        "limit": { "context": 204800, "output": 131072 }
      },
      "glm-5.2": {
        "id": "glm-5.2",
        "temperature": true,
        "limit": { "context": 204800, "output": 131072 }
      }
    }
  },
  "openai": {
    "id": "openai",
    "api": null,
    "models": {
      "gpt-5.6-sol": {
        "id": "gpt-5.6-sol",
        "temperature": false,
        "limit": { "context": 400000, "output": 128000 }
      }
    }
  },
  "blank-endpoint": {
    "id": "blank-endpoint",
    "api": "   ",
    "models": { "nowhere": { "id": "nowhere", "temperature": false } }
  },
  "quiet-vendor": {
    "id": "quiet-vendor",
    "api": "https://quiet.example/v1",
    "models": { "unspecified": { "id": "unspecified" } }
  }
}"#;
