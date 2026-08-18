//! Output-sink behaviour: `doctor` is diagnosis and must not turn a writer
//! failure into a gate failure.

use crate::cli::doctor::{DoctorArgs, run, run_to};

/// A sink that refuses every write with `BrokenPipe`, as a closed pipe does.
struct ClosedPipe;

impl std::io::Write for ClosedPipe {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "closed",
        ))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A sink that fails with something that is *not* a broken pipe.
struct BadDisk;

impl std::io::Write for BadDisk {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk on fire"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `drep doctor | head -5` closes the pipe under us. That is the reader's
/// choice, not a diagnostic failure - and `main.rs` maps any `Err` to exit 2,
/// so letting it through would contradict this command's one contract.
///
/// The `BadDisk` half is what makes the classification meaningful: without it,
/// "treat every write error as success" passes, and a genuinely failed report
/// would be indistinguishable from a delivered one.
#[test]
fn a_closed_pipe_is_not_a_failure_but_a_real_write_error_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.py"), "x = 1\n").expect("a.py");
    let args = DoctorArgs {
        path: dir.path().to_path_buf(),
        config: None,
    };

    // `run` owns the classification; `run_to` propagates, which is why the
    // pipe case is asserted through `run`.
    let mut sink = ClosedPipe;
    assert!(
        run_to(&mut sink, &args).is_err(),
        "run_to reports the write failure to its caller"
    );
    assert_eq!(
        run(&args).expect("stdout is a real sink here"),
        crate::Exit::Clean
    );

    let mut sink = BadDisk;
    let err = run_to(&mut sink, &args).expect_err("a real IO error must surface");
    assert!(
        !crate::cli::doctor::is_broken_pipe(&err),
        "a disk error is not a closed pipe"
    );
    let mut sink = ClosedPipe;
    let err = run_to(&mut sink, &args).expect_err("closed pipe");
    assert!(
        crate::cli::doctor::is_broken_pipe(&err),
        "and a closed pipe is"
    );
}
