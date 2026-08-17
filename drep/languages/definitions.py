"""The registered languages.

Adding a language is an entry here plus, if it has one, a tool output parser.
No control flow anywhere else in drep changes.

`config_files` is what makes a tool run at all: drep checks a project against
the style that project has *chosen*, so a repo with no eslint config gets no
eslint findings rather than a wall of default-preset complaints.
"""

from drep.languages.base import LanguageSupport, ToolSpec, registry

RUFF = ToolSpec(
    name="ruff",
    command=("ruff", "check", "--output-format", "json"),
    local_paths=("venv/bin/ruff", ".venv/bin/ruff"),
    config_files=("pyproject.toml", "ruff.toml", ".ruff.toml"),
)

ESLINT = ToolSpec(
    name="eslint",
    command=("eslint", "--format", "json"),
    local_paths=("node_modules/.bin/eslint",),
    config_files=(
        "eslint.config.js",
        "eslint.config.mjs",
        "eslint.config.cjs",
        ".eslintrc",
        ".eslintrc.js",
        ".eslintrc.cjs",
        ".eslintrc.json",
        ".eslintrc.yml",
        ".eslintrc.yaml",
    ),
)

TSC = ToolSpec(
    name="tsc",
    command=("tsc", "--noEmit", "--pretty", "false"),
    local_paths=("node_modules/.bin/tsc",),
    config_files=("tsconfig.json",),
    output_format="tsc",
    diagnostics_stream="stdout",
)

GOFMT = ToolSpec(
    name="gofmt",
    command=("gofmt", "-l"),
    local_paths=(),
    # go.mod is the marker that this is a Go module at all; gofmt has no
    # config of its own because its formatting is not configurable.
    config_files=("go.mod",),
    output_format="lines",
)

GO_VET = ToolSpec(
    name="go vet",
    # Not -json: that only emits JSON once the package compiles, and a package
    # that does not compile is exactly when vet has the most to say.
    command=("go", "vet"),
    local_paths=(),
    config_files=("go.mod",),
    output_format="position",
    diagnostics_stream="stderr",
)

CLIPPY = ToolSpec(
    name="clippy",
    command=("cargo", "clippy", "--message-format", "json", "--quiet"),
    local_paths=(),
    config_files=("Cargo.toml",),
    output_format="cargo",
)


PYTHON = LanguageSupport(
    name="python",
    display_name="Python",
    extensions=(".py",),
    tools=(RUFF,),
    conventions=(
        "Follows PEP 8 naming and structure",
        "Type hints on public APIs, and correct use of Optional/None",
        "Context managers for resources rather than manual cleanup",
        "Mutable default arguments, and late-binding closures in loops",
    ),
)

JAVASCRIPT = LanguageSupport(
    name="javascript",
    display_name="JavaScript",
    extensions=(".js", ".jsx", ".mjs", ".cjs"),
    tools=(ESLINT,),
    conventions=(
        "Unhandled promise rejections and missing await",
        "Sequential awaits in a loop where the work is independent",
        "var versus let/const, and accidental global scope",
        "Equality coercion (== versus ===)",
    ),
)

TYPESCRIPT = LanguageSupport(
    name="typescript",
    display_name="TypeScript",
    extensions=(".ts", ".tsx", ".mts", ".cts"),
    tools=(ESLINT, TSC),
    conventions=(
        "`any` where a real type is available, and unsafe casts",
        "Unhandled promise rejections and missing await",
        "Non-null assertions (!) that hide a genuine null case",
        "Sequential awaits in a loop where the work is independent",
    ),
)

GO = LanguageSupport(
    name="go",
    display_name="Go",
    extensions=(".go",),
    tools=(GOFMT, GO_VET),
    conventions=(
        "Errors ignored rather than checked and wrapped",
        "Goroutine leaks, and writes to a channel nobody reads",
        "defer inside a loop, and defer that never runs",
        "Data races on shared state without synchronisation",
    ),
)

RUST = LanguageSupport(
    name="rust",
    display_name="Rust",
    extensions=(".rs",),
    tools=(CLIPPY,),
    conventions=(
        "unwrap/expect on values that can legitimately be None or Err",
        "unsafe blocks, and whether their invariants are documented",
        "Unnecessary clones and allocations in hot paths",
        "Send/Sync correctness for types crossing threads",
    ),
)


for _language in (PYTHON, JAVASCRIPT, TYPESCRIPT, GO, RUST):
    registry.register(_language)
