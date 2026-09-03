//! The registered languages, split by ecosystem.
//!
//! Adding a language is an entry in its ecosystem file (plus a new file for
//! a new ecosystem) and, if it has one, a tool output parser. No control flow
//! anywhere else in drep changes.
//!
//! `config_files` is what makes a tool run at all: drep checks a project against
//! the style that project has *chosen*, so a repo with no eslint config gets no
//! eslint findings rather than a wall of default-preset complaints.
//!
//! Every ecosystem's statics are re-exported from here so the module path a
//! consumer uses (`definitions::RUFF`) is unchanged by the split.

mod c_family;
mod docker;
mod elixir;
mod go;
mod javascript;
mod jvm;
mod php;
mod python;
mod ruby;
mod rust;
mod shell;
mod sql;
mod swift;
mod terraform;

pub use c_family::{C, CPP, CPPCHECK, CSHARP, DOTNET_FORMAT};
pub use docker::{DOCKER, HADOLINT};
pub use elixir::{CREDO, ELIXIR};
pub use go::{GO, GO_VET, GOFMT};
pub use javascript::{ESLINT, JAVASCRIPT, SVELTE, TSC, TYPESCRIPT, VUE};
pub use jvm::{CHECKSTYLE, GROOVY, JAVA, KOTLIN, KTLINT, SCALA};
pub use php::{PHP, PHPCS};
pub use python::{PYTHON, RUFF};
pub use ruby::{RUBOCOP, RUBY};
pub use rust::{CLIPPY, RUST_LANG};
pub use shell::{SHELL, SHELLCHECK};
pub use sql::{SQL, SQLFLUFF};
pub use swift::{SWIFT, SWIFTLINT};
pub use terraform::{TERRAFORM, TFLINT};

use super::spec::LanguageSupport;

/// Every registered language, in registration order.
///
/// The order is the order `doctor` reports languages in, so it is part of
/// the output contract: append new entries rather than inserting.
pub static ALL_LANGUAGES: &[&LanguageSupport] = &[
    &PYTHON,
    &JAVASCRIPT,
    &TYPESCRIPT,
    &GO,
    &RUST_LANG,
    &JAVA,
    &KOTLIN,
    &SCALA,
    &GROOVY,
    &SHELL,
    &SWIFT,
    &C,
    &CPP,
    &CSHARP,
    &RUBY,
    &PHP,
    &VUE,
    &SVELTE,
    &TERRAFORM,
    &ELIXIR,
    &SQL,
    &DOCKER,
];
