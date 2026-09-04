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
use std::sync::LazyLock;

/// Every registered language, in registration order.
///
/// Assembled from each ecosystem file's own `FAMILY` slice rather than
/// maintained here as a hand-written list. A static can be defined and
/// re-exported above while never being registered, and no test could see the
/// omission - the same failure class as a test file no `mod` declares. With
/// the list built beside the definitions, leaving a language out means
/// leaving it out of the file that defines it.
///
/// The order is the order `doctor` reports languages in, so it remains part
/// of the output contract - but the contract is now two-level: families in
/// the order listed below, and within a family the order its own file writes.
/// Appending a language to its family therefore moves nothing, while adding a
/// family appends only at the position it is listed here. Assembling this way
/// reordered the flat list once, on the commit that introduced it, by moving
/// Vue and Svelte up beside TypeScript where they belong; the previous rule
/// ("append rather than insert") described the hand-written list and would now
/// forbid the grouping that replaced it.
///
/// `all_languages_returns_every_registered_language` pins the resulting
/// sequence in full, so a reorder is a visible test change rather than a
/// silent one.
pub(crate) static ALL_LANGUAGES: LazyLock<Vec<&'static LanguageSupport>> = LazyLock::new(|| {
    [
        python::FAMILY,
        javascript::FAMILY,
        go::FAMILY,
        rust::FAMILY,
        jvm::FAMILY,
        shell::FAMILY,
        swift::FAMILY,
        c_family::FAMILY,
        ruby::FAMILY,
        php::FAMILY,
        terraform::FAMILY,
        elixir::FAMILY,
        sql::FAMILY,
        docker::FAMILY,
    ]
    .concat()
});
