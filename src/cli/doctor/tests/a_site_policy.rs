//! The `Site policy:` block: the one line an operator needs when a repository
//! behaves differently on one machine than on another.
//!
//! Three states, all reported and none fatal. The broken state is the reason
//! this block exists at all: `drep check` refuses to run on a policy file it
//! cannot load, and `doctor` is the command someone runs to find out why.

use std::path::Path;

use crate::cli::doctor::{DoctorArgs, run_at};

fn args(dir: &Path) -> DoctorArgs {
    DoctorArgs {
        path: dir.to_path_buf(),
        config: None,
    }
}

/// Run `doctor` against `dir` with a temporary store and the named policy file.
async fn report_with_site(dir: &Path, site_path: &Path) -> String {
    let mut out = Vec::new();
    let exit = run_at(&mut out, &args(dir), &dir.join("auth.toml"), site_path)
        .await
        .expect("run_at");
    assert_eq!(
        exit,
        crate::Exit::Clean,
        "a policy diagnosis is never a gate failure"
    );
    String::from_utf8(out).expect("utf8")
}

/// One enabled provider, leaving `max_concurrent` at its default.
fn write_provider(dir: &Path) {
    std::fs::write(
        dir.join("drep.toml"),
        "[[llm]]\nendpoint = \"http://e/v1\"\nmodel = \"m\"\n",
    )
    .expect("config");
}

fn write_source(dir: &Path) {
    std::fs::write(dir.join("a.py"), "x = 1\n").expect("source");
}

#[tokio::test]
async fn no_site_file_says_so_and_names_the_path_it_looked_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_source(dir.path());
    write_provider(dir.path());
    let site = dir.path().join("absent-site.toml");

    let report = report_with_site(dir.path(), &site).await;

    assert!(report.contains("Site policy:"), "got {report}");
    assert!(
        report.contains(&site.display().to_string()),
        "an operator with nowhere to install policy has learned nothing; got {report}"
    );
}

#[tokio::test]
async fn a_site_file_in_effect_is_named_with_its_ceiling() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_source(dir.path());
    write_provider(dir.path());
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "max_concurrent_ceiling = 4\n").expect("site.toml");

    let report = report_with_site(dir.path(), &site).await;

    assert!(
        report.contains(&format!("in effect from {}", site.display())),
        "a report silent about a policy that is changing behaviour is the \
         report this block exists to replace; got {report}"
    );
    assert!(report.contains("max_concurrent ceiling: 4"), "got {report}");
}

/// The clamp is shown on the provider it changes, and only on that one.
#[tokio::test]
async fn a_clamped_provider_says_so_on_its_own_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("source");
    std::fs::write(
        dir.path().join("drep.toml"),
        "[[llm]]\nendpoint = \"http://high/v1\"\nmodel = \"high\"\nmax_concurrent = 8\n\n\
         [[llm]]\nendpoint = \"http://low/v1\"\nmodel = \"low\"\nmax_concurrent = 2\n",
    )
    .expect("config");
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "max_concurrent_ceiling = 4\n").expect("site.toml");

    let report = report_with_site(dir.path(), &site).await;

    assert!(
        report.contains("max_concurrent: 8 lowered to 4"),
        "got {report}"
    );
    assert_eq!(
        report.matches("lowered to").count(),
        1,
        "the entry already below the ceiling was not clamped, so saying it was \
         is a report of a change that did not happen; got {report}"
    );
}

/// `doctor` describes the fatality rather than propagating it.
///
/// Propagating would suppress everything else the report had to say, in the one
/// command an operator runs to diagnose exactly this refusal - the same reasoning
/// the unreadable-auth-store arm already follows.
#[tokio::test]
async fn a_broken_site_file_is_described_rather_than_failing_doctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_source(dir.path());
    write_provider(dir.path());
    let site = dir.path().join("site.toml");
    std::fs::write(&site, "not toml at all\n").expect("site.toml");

    let report = report_with_site(dir.path(), &site).await;

    assert!(report.contains(&site.display().to_string()), "got {report}");
    assert!(
        report.contains("refuses to run"),
        "the reader has to be told that `drep check` will not run until this is \
         fixed; got {report}"
    );
    assert!(
        report.contains("LLM analysis"),
        "and the rest of the report still has to arrive; got {report}"
    );
}

/// One ordering, stated once, in both report shapes.
///
/// The two branches used to call the LLM block separately, so this is what
/// catches them drifting - and it catches the policy block being inserted ahead
/// of the header that `a_no_files` pins exactly.
#[tokio::test]
async fn the_site_block_precedes_the_llm_block_with_and_without_source_files() {
    for source in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        if source {
            write_source(dir.path());
        }
        write_provider(dir.path());
        let site = dir.path().join("site.toml");
        std::fs::write(&site, "max_concurrent_ceiling = 4\n").expect("site.toml");

        let report = report_with_site(dir.path(), &site).await;

        let policy = report.find("Site policy:");
        let llm = report.find("LLM analysis");
        assert!(
            policy.is_some() && policy < llm,
            "with source = {source}, the policy that governs the chain is read \
             before the chain it governs; got {report}"
        );
        assert!(
            report.starts_with("drep in "),
            "with source = {source}, the header still comes first; got {report}"
        );
    }
}
