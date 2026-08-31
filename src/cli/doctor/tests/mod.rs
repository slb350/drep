//! Acceptance tests for `drep doctor` (Part A).
//!
//! Each test runs the command against a `TempDir` and asserts on the captured
//! string. No subprocess - the command takes a `&mut dyn Write` precisely so
//! the tests can read what would otherwise go to stdout.

mod a_codex;
mod a_headers;
mod a_key_command;
mod a_languages;
mod a_llm_section;
mod a_no_files;
mod a_site_policy;
mod a_skipped_vs_missing;
mod a_special_cases;

/// Run `doctor` with the auth store and the policy file scoped to `dir`.
///
/// Every test here goes through this rather than `run_to`, which resolves
/// `crate::auth::default_path()` and `crate::config::site::default_path()` - the
/// real ones. Those are parameters of `run_at` precisely so no test reads machine
/// state, and five of these files called `run_to` anyway: on a machine with a
/// policy installed they read it, and a `refuse_markers` entry there changes what
/// they assert. A suite whose result depends on the developer's `/etc` is a suite
/// that passes or fails for a reason unrelated to the code.
///
/// `absent-site.toml` is named rather than written, because "no policy installed"
/// is the state these tests want and a missing file is exactly how the loader
/// spells it.
async fn run_scoped<W: std::io::Write>(
    out: &mut W,
    args: &super::DoctorArgs,
    dir: &std::path::Path,
) -> anyhow::Result<crate::Exit> {
    super::run_at(
        out,
        args,
        &crate::cli::MachineFiles {
            auth: &dir.join("auth.toml"),
            policy: &dir.join("absent-site.toml"),
        },
    )
    .await
}

/// Capture a doctor report against one explicit policy path.
async fn report_scoped_with_policy(dir: &std::path::Path, policy: &std::path::Path) -> String {
    let mut out = Vec::new();
    let exit = super::run_at(
        &mut out,
        &super::DoctorArgs {
            path: dir.to_path_buf(),
            config: None,
        },
        &crate::cli::MachineFiles {
            auth: &dir.join("auth.toml"),
            policy,
        },
    )
    .await
    .expect("doctor remains diagnostic");
    assert_eq!(exit, crate::Exit::Clean, "doctor reports but never gates");
    String::from_utf8(out).expect("doctor output is UTF-8")
}

/// Capture a doctor report with an injected Codex readiness probe.
async fn report_scoped_with_codex(
    dir: &std::path::Path,
    policy: &std::path::Path,
    probe: &dyn Fn() -> Result<crate::llm::codex::CodexStatus, String>,
) -> String {
    let mut out = Vec::new();
    let exit = super::run_at_with_codex(
        &mut out,
        &super::DoctorArgs {
            path: dir.to_path_buf(),
            config: None,
        },
        &crate::cli::MachineFiles {
            auth: &dir.join("auth.toml"),
            policy,
        },
        probe,
    )
    .await
    .expect("doctor remains diagnostic");
    assert_eq!(exit, crate::Exit::Clean, "doctor reports but never gates");
    String::from_utf8(out).expect("doctor output is UTF-8")
}
