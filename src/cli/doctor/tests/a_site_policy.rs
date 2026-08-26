//! The `Site policy:` block: the one line an operator needs when a repository
//! behaves differently on one machine than on another.
//!
//! Three states, all reported and none fatal. The broken state is the reason
//! this block exists at all: `drep check` refuses to run on a policy file it
//! cannot load, and `doctor` is the command someone runs to find out why.

use std::path::Path;

use crate::test_support::write_site_policy;

/// Run `doctor` against `dir` with a temporary store and the named policy file.
async fn report_with_site(dir: &Path, site_path: &Path) -> String {
    super::report_scoped_with_policy(dir, site_path).await
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

/// The refusal is reported where the policy is, and named.
///
/// `drep check` exits 2 in this repository. An operator who has just watched that
/// happen runs `doctor`, and this block is where the answer has to be.
#[tokio::test]
async fn a_marked_repository_says_semantic_review_is_refused_here() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    write_source(dir.path());
    write_provider(dir.path());
    let site = write_site_policy(dir.path(), &[".drep-no-llm"]);
    std::fs::write(dir.path().join(".drep-no-llm"), "").expect("marker");

    let report = report_with_site(dir.path(), &site).await;

    assert!(
        report.contains("refuse_markers: .drep-no-llm"),
        "got {report}"
    );
    assert!(
        report.contains("refused"),
        "an operator staring at exit 2 needs the word; got {report}"
    );
    assert!(
        report.contains(".drep-no-llm is present"),
        "and which file it was; got {report}"
    );
}

/// Doctor describes the repositories holding discovered source, not only the
/// directory it was pointed at.
///
/// A bare check walks nested repositories too. Reporting the unmarked outer
/// root as permitted would let doctor spend credentials for a run that the gate
/// refuses as soon as it reaches the marked inner source.
#[tokio::test]
async fn a_marked_nested_repository_is_refused_in_the_doctor_report() {
    let outer = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(outer.path());
    write_provider(outer.path());
    write_source(outer.path());
    let inner = outer.path().join("nested");
    std::fs::create_dir(&inner).expect("nested repo");
    crate::test_support::git_init(&inner);
    std::fs::write(inner.join("inner.py"), "y = 2\n").expect("inner source");
    std::fs::write(inner.join(".drep-no-llm"), "").expect("inner marker");
    let site = write_site_policy(outer.path(), &[".drep-no-llm"]);

    let report = report_with_site(outer.path(), &site).await;

    assert!(
        report.contains(&inner.join(".drep-no-llm").display().to_string()),
        "doctor and check disagreed about the source repositories: {report}"
    );
    assert!(
        report.contains("semantic review is refused"),
        "got {report}"
    );
}

/// The discriminating half: a configured marker that is absent refuses nothing,
/// and the report has to say that too.
///
/// Without it, "print the refusal line whenever `refuse_markers` is set" passes
/// the test above. The positive wording is asserted, not just the absence of the
/// refusal: an arm that printed nothing at all satisfies `!contains("is present")`
/// and leaves an operator reading `refuse_markers: .drep-no-llm` with no statement
/// of what it does here, which is the ambiguity the effect line exists to remove.
#[tokio::test]
async fn a_configured_marker_that_is_absent_is_reported_as_not_refusing() {
    let dir = tempfile::tempdir().expect("tempdir");
    crate::test_support::git_init(dir.path());
    write_source(dir.path());
    write_provider(dir.path());
    let site = write_site_policy(dir.path(), &[".drep-no-llm"]);

    let report = report_with_site(dir.path(), &site).await;

    assert!(
        report.contains("refuse_markers: .drep-no-llm"),
        "got {report}"
    );
    assert!(
        report.contains("none of those files is here, so review runs"),
        "the list alone leaves the operator guessing at its effect; got {report}"
    );
    assert!(
        !report.contains("is present"),
        "reporting a refusal that is not happening is worse than silence; got {report}"
    );
}

/// A policy that cannot be evaluated is described, and `doctor` still finishes.
///
/// `drep check` fails closed here, so this is again the command someone runs to
/// find out why - and failing out would suppress the rest of the answer.
///
/// The fixture denies git a root through `git_unresolvable` rather than by being a
/// plain temporary directory: whether a `TempDir` sits inside a repository is a
/// property of the developer's machine, and on one whose `TMPDIR` does, this test
/// would fail for a reason unrelated to the code.
#[tokio::test]
async fn a_policy_that_cannot_be_evaluated_is_described_rather_than_failing_doctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_source(dir.path());
    write_provider(dir.path());
    crate::test_support::git_unresolvable(dir.path());
    let site = write_site_policy(dir.path(), &[".drep-no-llm"]);

    let report = report_with_site(dir.path(), &site).await;

    assert!(
        report.contains("could not be resolved"),
        "a policy naming markers outside a repository cannot be evaluated at \
         all, and the report has to say so; got {report}"
    );
    assert!(
        report.contains("LLM analysis"),
        "and the rest of the report still has to arrive; got {report}"
    );
}
