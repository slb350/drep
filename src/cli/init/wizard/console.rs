//! The wizard's view of a terminal, and the real one.
//!
//! Split out of `wizard.rs` because it is the seam every test replaces and it
//! shares nothing with the flow above it: [`Console`] is three questions, and
//! [`Terminal`] is the only implementation that touches stdin. Keeping them
//! here leaves `wizard.rs` to the part that decides what to ask.

use anyhow::{Result, anyhow};

/// The terminal, or a scripted stand-in.
pub trait Console {
    /// Print a line.
    fn say(&mut self, line: &str) -> Result<()>;

    /// Ask a question and read one line.
    ///
    /// `default` is shown in the prompt and returned when the answer is empty,
    /// so pressing Enter accepts it.
    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<String>;

    /// Ask for a secret and read one line without echoing it.
    ///
    /// An empty answer is legitimate and means "skip", so this cannot signal
    /// refusal by returning an error.
    fn ask_secret(&mut self, question: &str) -> Result<String>;
}

/// The real terminal.
///
/// Reads lines from stdin and secrets through `rpassword`, which turns echo off
/// for the duration of the read. A pasted key would otherwise sit in the
/// terminal's scrollback, which is the one place drep takes care not to put a
/// credential anywhere else - `LlmConfig`, `LlmClient` and `AuthStore` all
/// hand-write `Debug` for the same reason.
pub struct Terminal<'a, W: std::io::Write> {
    out: &'a mut W,
}

impl<'a, W: std::io::Write> Terminal<'a, W> {
    /// Wrap `out` as the wizard's console.
    pub fn new(out: &'a mut W) -> Self {
        Self { out }
    }
}

impl<W: std::io::Write> Console for Terminal<'_, W> {
    fn say(&mut self, line: &str) -> Result<()> {
        writeln!(self.out, "{line}")?;
        Ok(())
    }

    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<String> {
        match default {
            Some(value) => write!(self.out, "{question} [{value}]: ")?,
            None => write!(self.out, "{question}: ")?,
        }
        self.out.flush()?;

        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line)?;
        // End of input mid-wizard. Returning the default would silently accept
        // choices nobody made, so it is an error naming what happened.
        if read == 0 {
            return Err(anyhow!("input ended while `drep init` was still asking"));
        }

        let answer = line.trim();
        Ok(match (answer.is_empty(), default) {
            (true, Some(value)) => value.to_string(),
            _ => answer.to_string(),
        })
    }

    fn ask_secret(&mut self, question: &str) -> Result<String> {
        use std::io::IsTerminal;

        write!(self.out, "{question}: ")?;
        self.out.flush()?;

        // `rpassword` turns echo off on the controlling terminal, which means
        // opening `/dev/tty` - and that fails outright when there is none,
        // rather than degrading. A piped stdin has nothing to echo in the first
        // place: the data is not being typed, so there is no echo to suppress
        // and a plain read is both correct and the only thing that works.
        let secret = if std::io::stdin().is_terminal() {
            rpassword::read_password()?
        } else {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line.trim_end_matches(['\n', '\r']).to_string()
        };

        // The typed newline was consumed by whichever branch ran, so the next
        // line would otherwise start on the same row as the prompt.
        writeln!(self.out)?;
        Ok(secret)
    }
}
