use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use drep::Exit;
use drep::cli::{self, Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        // Purely synchronous commands never boot a Tokio thread pool.
        Command::Auth(args) => cli::auth::run(&args),
        Command::Acknowledge(args) => cli::acknowledge::run(&args, Path::new(".")),
        Command::LintDocs(args) if args.staged => {
            run_async(cli::lint_docs::run_staged(&args, Path::new(".")))
        }
        Command::LintDocs(args) => cli::lint_docs::run_local(&args, Path::new(".")),
        other => run_async(cli::run(Cli { command: other })),
    };
    match result {
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

fn run_async(
    future: impl std::future::Future<Output = anyhow::Result<Exit>>,
) -> anyhow::Result<Exit> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
