//! The C family: cppcheck over C and C++, `dotnet format` over C#.
//!
//! C and C++ are separate languages - they claim disjoint extensions and the
//! semantic reviewer's conventions differ - while sharing the one checker:
//! cppcheck analyzes both, and a project's build files decide how it runs.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// C and C++ deterministic checker.
///
/// The config markers are project build files rather than a cppcheck config,
/// following the gofmt/`go.mod` and clippy/`Cargo.toml` precedent: cppcheck
/// has no conventional config file of its own, and the presence of a build
/// system is what says "this project's C is checked here".
///
/// SARIF goes to **stderr**: cppcheck leaves stdout nearly empty (progress
/// chatter only), so reading stdout reports every C file clean.
pub static CPPCHECK: ToolSpec = ToolSpec {
    name: "cppcheck",
    command: &[
        "cppcheck",
        "--output-format=sarif",
        "--enable=warning,style",
    ],
    local_paths: &[],
    config_files: &[
        "CMakeLists.txt",
        "Makefile",
        "meson.build",
        "compile_commands.json",
    ],
    config_flag: None,
    output_format: "sarif",
    diagnostics_stream: "stderr",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// C# deterministic checker.
///
/// `dotnet format` checks a *project*, not a file list, so it runs bare and
/// its findings are narrowed to the files being checked afterwards - exactly
/// as tsc's and clippy's are. The ceiling covers an MSBuild project load,
/// which can dominate the run on a large solution.
pub static DOTNET_FORMAT: ToolSpec = ToolSpec {
    name: "dotnet format",
    command: &["dotnet", "format", "--verify-no-changes", "--no-restore"],
    local_paths: &[],
    // The project marker, not the style file: `dotnet format` must run from
    // the directory holding the solution or project, and `.editorconfig`
    // names neither. The same choice gofmt makes with `go.mod` and clippy
    // with `Cargo.toml`; `.editorconfig` still supplies the rules when the
    // project has one, and .NET's own defaults when it does not.
    config_files: &["*.sln", "*.csproj"],
    config_flag: None,
    output_format: "msbuild",
    diagnostics_stream: "stdout",
    timeout_secs: 600,
    timeout_context: Some(", including its MSBuild project load"),
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: false,
};

/// C language entry.
pub static C: LanguageSupport = LanguageSupport {
    name: "c",
    display_name: "C",
    extensions: &[".c", ".h"],
    filenames: &[],
    tools: &[&CPPCHECK],
    conventions: &[
        "Buffer overruns and off-by-one indexing into fixed arrays",
        "Use-after-free, double free, and leaks on early error paths",
        "Unchecked return values from allocation and system calls",
        "Signedness confusion and integer overflow in arithmetic",
        "Data races on shared state without synchronisation",
    ],
    vendored_dirs: &[],
};

/// C++ language entry.
///
/// `.h` stays with C and `.hpp`/`.hh`/`.hxx` with C++: a header's language
/// is convention, not syntax, and the extensions are how every build system
/// in practice distinguishes them.
pub static CPP: LanguageSupport = LanguageSupport {
    name: "cpp",
    display_name: "C++",
    extensions: &[".cpp", ".hpp", ".cc", ".hh", ".cxx", ".hxx"],
    filenames: &[],
    tools: &[&CPPCHECK],
    conventions: &[
        "Dangling references and iterators into reallocated containers",
        "Ownership confusion between raw and smart pointers",
        "Missing virtual destructors on polymorphic base classes",
        "Uninitialised members and reads from moved-from state",
        "Templates instantiated with types that do not satisfy their assumptions",
    ],
    vendored_dirs: &[],
};

/// C# language entry.
///
/// No `vendored_dirs`, for the reason `JVM_VENDORED_DIRS` leaves out `out`:
/// `files::is_ignored_dir` consults the union across every language, so an
/// entry here skips that directory in repositories with no C# in them at all.
/// MSBuild's `bin` and `obj` are machine-generated and therefore gitignored in
/// practice, which the walker already honors on its own - while `bin/` holding
/// real checked-in scripts is a convention across several ecosystems. Listing
/// it hid `bin/deploy.sh` from the newly registered Shell language, and the
/// `RUBOCOP` spec in `ruby.rs` looks for `bin/rubocop`. The cost of listing them
/// is a silent skip; the benefit is a directory git already ignores.
pub static CSHARP: LanguageSupport = LanguageSupport {
    name: "csharp",
    display_name: "C#",
    extensions: &[".cs"],
    filenames: &[],
    tools: &[&DOTNET_FORMAT],
    conventions: &[
        "async void, and tasks that are never awaited",
        "IDisposable not disposed on every path",
        "Null dereferences the nullable flow analysis would catch",
        "Closures capturing a loop variable's stale value",
        "Struct copies where a reference was intended",
    ],
    vendored_dirs: &[],
};
