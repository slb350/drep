//! Unit and process-boundary tests for the Codex backend.

#[cfg(unix)]
mod client;
mod command;
mod diagnostics;
mod events;

#[cfg(unix)]
fn probe_and_stop_process(pid: &str) -> bool {
    let running = std::process::Command::new("/bin/kill")
        .args(["-0", pid])
        .status()
        .expect("probe grandchild")
        .success();
    if running {
        let _ = std::process::Command::new("/bin/kill").arg(pid).status();
    }
    running
}
