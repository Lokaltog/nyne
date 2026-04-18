//! Test target symbols used across multiple integration test files.
//!
//! Centralizing these avoids multi-file updates when a target is renamed or
//! moved. Each submodule picks a target that exercises a specific set of
//! VFS features:
//!
//! - [`rust`] — `Cli` struct with docstring, decorators, and a sibling impl
//!   that nests a method child. Used for tree-sitter-backed symbol reads,
//!   per-symbol git nodes, and file-level git tests.
//! - [`lsp`] — `Provider` trait with many implementations and references
//!   across every plugin crate. Used for LSP-backed nodes (references,
//!   implementations, rename previews).

/// Rust source target with rich tree-sitter structure.
pub mod rust {
    /// Path to the target Rust file (relative to mount root).
    pub const FILE: &str = "nyne/src/cli/mod.rs";
    /// Top-level struct with a docstring and `#[derive]`/`#[command]` attributes.
    pub const SYMBOL: &str = "Cli";
    /// Sibling impl block that exposes a nested method as a child symbol.
    pub const IMPL: &str = "Command~Impl";
    /// Method nested inside [`IMPL`] — used to test nested child access.
    pub const NESTED: &str = "run";
}

/// LSP target with broad call-graph coverage.
///
/// Two anchor points:
///
/// - [`SYMBOL`] — `Provider` trait declaration. Has many `references` and
///   `implementation` entries across `plugins/`, but as a trait declaration
///   it has **no callers and no dependencies** (only its methods do). Use
///   this for `REFERENCES.md` / `IMPLEMENTATION.md` / `DECLARATION.md` /
///   `DEFINITION.md` / `DOC.md` / rename / actions.
/// - [`METHOD_FILE`] + [`METHOD_SYMBOL`] — a concrete `Provider::accept`
///   impl with both incoming callers and outgoing deps that include
///   `plugins/` paths. Use this for `CALLERS.md` / `DEPS.md`.
pub mod lsp {
    /// Path to the target Rust file (relative to mount root).
    pub const FILE: &str = "nyne/src/router/pipeline/provider.rs";
    /// Widely-implemented trait referenced from every plugin crate.
    pub const SYMBOL: &str = "Provider";

    /// Path to the file containing [`METHOD_SYMBOL`].
    pub const METHOD_FILE: &str = "plugins/cache/src/provider/mod.rs";
    /// Concrete `Provider::accept` impl with both callers and dependencies
    /// that span `plugins/` — usable for both `CALLERS.md` and `DEPS.md`
    /// gating assertions.
    pub const METHOD_SYMBOL: &str = "Provider_for_CacheProvider@/accept";
}
