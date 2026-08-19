//! A2: a directory with no source files drep recognises says so, skips the
//! language and tool sections - and still reports the LLM configuration.

use crate::cli::doctor::{DoctorArgs, run_to};

fn args(path: &std::path::Path) -> DoctorArgs {
    DoctorArgs {
        path: path.to_path_buf(),
        config: None,
    }
}

/// The sections that describe *code* are skipped; the one that describes
/// *configuration* is not.
///
/// The original spec returned early here, which answered "is my model set up?"
/// with silence in precisely the repository where a new user is most likely to
/// be asking - a docs-only tree, or one whose languages drep does not
/// register. The two halves are asserted together because dropping either
/// makes the other trivially satisfiable.
#[test]
fn no_recognised_files_skips_the_code_sections_but_still_reports_the_llm() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("README.md"), "# readme\n").expect("README");
    std::fs::write(dir.path().join("notes.txt"), "notes\n").expect("notes");

    let root = dir.path().canonicalize().expect("canonical");
    let mut out = Vec::new();
    let exit = run_to(&mut out, &args(dir.path())).expect("run_to");
    let rendered = String::from_utf8(out).expect("utf8");

    assert_eq!(exit, crate::Exit::Clean);

    let header = format!(
        "drep in {}\n{}\n\nNo source files drep recognises were found here.\n",
        root.display(),
        "=".repeat(60),
    );
    assert!(
        rendered.starts_with(&header),
        "header and sentence come first, exactly; rendered:\n{rendered}"
    );

    for absent in ["Languages found", "Deterministic checks"] {
        assert!(
            !rendered.contains(absent),
            "there is no code here, so `{absent}` has nothing to say; rendered:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("LLM analysis"),
        "the LLM section is the question a new user most needs answered; rendered:\n{rendered}"
    );
    assert!(
        rendered.contains("Run `drep init`"),
        "and with no drep.toml it should say what to do; rendered:\n{rendered}"
    );
}
