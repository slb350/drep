//! The JavaScript/TypeScript ecosystem, plus the component frameworks that
//! reuse eslint: Vue and Svelte ship no linter of their own, so their single
//! files are `.vue`/`.svelte` templates and script inside them, checked by
//! the project's eslint config when it has the parser plugins.

use crate::languages::spec::{DEFAULT_TOOL_TIMEOUT_SECS, LanguageSupport, ToolSpec};

/// Package installs and build outputs shared by the JavaScript ecosystem.
/// npm and its rivals install into `node_modules`; Next.js and Nuxt write
/// `.next` and `.nuxt` as their build directories. Declared once rather than
/// repeated across four entries that can never legitimately disagree.
static JS_VENDORED_DIRS: &[&str] = &["node_modules", ".next", ".nuxt"];

/// JavaScript deterministic checker.
pub static ESLINT: ToolSpec = ToolSpec {
    name: "eslint",
    command: &["eslint", "--format", "json"],
    local_paths: &["node_modules/.bin/eslint"],
    config_files: &[
        "eslint.config.js",
        "eslint.config.mjs",
        "eslint.config.cjs",
        ".eslintrc",
        ".eslintrc.js",
        ".eslintrc.cjs",
        ".eslintrc.json",
        ".eslintrc.yml",
        ".eslintrc.yaml",
    ],
    config_flag: None,
    output_format: "json",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: false,
    serial_in_repository: false,
    accepts_files: true,
};

/// TypeScript's compiler-as-checker. Streams diagnostics to stdout.
pub static TSC: ToolSpec = ToolSpec {
    name: "tsc",
    command: &["tsc", "--noEmit", "--pretty", "false"],
    local_paths: &["node_modules/.bin/tsc"],
    config_files: &["tsconfig.json"],
    config_flag: None,
    output_format: "tsc",
    diagnostics_stream: "stdout",
    timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
    timeout_context: None,
    establishes_compilation: true,
    serial_in_repository: false,
    // Passing source files makes tsc ignore tsconfig.json. Run the configured
    // project and filter its diagnostics back to the requested files.
    accepts_files: false,
};

/// JavaScript language entry.
pub static JAVASCRIPT: LanguageSupport = LanguageSupport {
    name: "javascript",
    display_name: "JavaScript",
    extensions: &[".js", ".jsx", ".mjs", ".cjs"],
    filenames: &[],
    tools: &[&ESLINT],
    conventions: &[
        "Unhandled promise rejections and missing await",
        "Sequential awaits in a loop where the work is independent",
        "var versus let/const, and accidental global scope",
        "Equality coercion (== versus ===)",
    ],
    vendored_dirs: JS_VENDORED_DIRS,
};

/// TypeScript language entry.
pub static TYPESCRIPT: LanguageSupport = LanguageSupport {
    name: "typescript",
    display_name: "TypeScript",
    extensions: &[".ts", ".tsx", ".mts", ".cts"],
    filenames: &[],
    tools: &[&ESLINT, &TSC],
    conventions: &[
        "`any` where a real type is available, and unsafe casts",
        "Unhandled promise rejections and missing await",
        "Non-null assertions (!) that hide a genuine null case",
        "Sequential awaits in a loop where the work is independent",
    ],
    vendored_dirs: JS_VENDORED_DIRS,
};

/// Vue language entry.
///
/// Reuses the project's eslint: a `.vue` file is script plus template, and
/// eslint with the Vue plugin is the checker a Vue project configures.
/// There is no separate Vue tool to declare.
pub static VUE: LanguageSupport = LanguageSupport {
    name: "vue",
    display_name: "Vue",
    extensions: &[".vue"],
    filenames: &[],
    tools: &[&ESLINT],
    conventions: &[
        "Reactive state mutated from outside the component that owns it",
        "v-for without a stable key, and keys that identify the wrong row",
        "Watchers whose side effects retrigger themselves",
        "Props mutated directly instead of emitted as events",
        "Computed properties hiding side effects",
    ],
    vendored_dirs: JS_VENDORED_DIRS,
};

/// Svelte language entry.
///
/// Same arrangement as Vue: eslint with the Svelte plugin is the
/// project-configured checker, so Svelte shares its entry.
pub static SVELTE: LanguageSupport = LanguageSupport {
    name: "svelte",
    display_name: "Svelte",
    extensions: &[".svelte"],
    filenames: &[],
    tools: &[&ESLINT],
    conventions: &[
        "Reactive statements with dependencies they do not declare",
        "Stores subscribed but never unsubscribed on destroy",
        "State mutated from outside the component tree",
        "Derived values recomputed on unrelated changes",
    ],
    vendored_dirs: JS_VENDORED_DIRS,
};
