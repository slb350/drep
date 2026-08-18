use std::process::ExitCode;

use clap::Parser;
use drep::Exit;
use drep::cli::{self, Cli};

#[tokio::main]
async fn main() -> ExitCode {
    match cli::run(Cli::parse()).await {
        Ok(exit) => exit.into(),
        // An error means analysis did not complete, which is exit 2 and never
        // exit 0. Reporting an unanalyzed run as clean is the one failure a
        // commit gate must not have.
        Err(err) => {
            eprintln!("drep: {err:#}");
            Exit::Unanalyzed.into()
        }
    }
}
