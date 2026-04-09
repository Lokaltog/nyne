//! Shared template partials and macros registered into every engine.
//!
//! Both the provider-side engine (used by system prompt, agent, and
//! skill rendering) and the hook-side engine (used by per-script
//! context messages) call [`register_shared_partials`] on construction.
//! Registering once here is the single source of truth for:
//!
//! - VFS path globals (`FILE_OVERVIEW`, `FILE_CALLERS`, …)
//! - `macros/paths` — path-fragment macros (`sym`, `sym_md`, `at_line`, …)
//! - `macros/hints` — prose hint macros (`edit_via_sym`, `vfs_analysis`, …)
//! - `shared/vfs-*` — content blocks composed into surface templates
//!
//! Individual surface templates (system prompt, agent, skills, hooks)
//! use `{% include "shared/..." %}` and `{% import "macros/..." %}` to
//! pull these in.

use nyne::templates::HandleBuilder;

/// Register VFS globals, macros, and shared content partials into the engine.
///
/// Call this once per engine, before registering the surface templates
/// that include the shared partials or import the macros.
pub fn register_shared_partials(b: &mut HandleBuilder) {
    let engine = b.engine_mut();
    engine.add_global("FILE_OVERVIEW", "OVERVIEW.md");
    engine.add_global("FILE_CALLERS", "CALLERS.md");
    engine.add_global("FILE_DEPS", "DEPS.md");
    engine.add_global("FILE_REFERENCES", "REFERENCES.md");
    engine.add_global("FILE_IMPLEMENTATION", "IMPLEMENTATION.md");
    // `ext` is a filetype placeholder used by surface templates. Hook
    // templates override this with the concrete extension from their
    // render context; surface templates fall back to the generic value.
    engine.add_global("ext", "<ext>");

    // Macros
    b.register_partial("macros/paths", include_str!("templates/macros/paths.j2"));
    b.register_partial("macros/hints", include_str!("templates/macros/hints.j2"));

    // Shared content blocks — composed into system prompt, agent, skills.
    b.register_partial("shared/vfs-intro", include_str!("templates/shared/vfs-intro.md.j2"));
    b.register_partial("shared/vfs-rules", include_str!("templates/shared/vfs-rules.md.j2"));
    b.register_partial(
        "shared/vfs-paths-table",
        include_str!("templates/shared/vfs-paths-table.md.j2"),
    );
    b.register_partial("shared/vfs-reading", include_str!("templates/shared/vfs-reading.md.j2"));
    b.register_partial("shared/vfs-writing", include_str!("templates/shared/vfs-writing.md.j2"));
    b.register_partial(
        "shared/vfs-batch-edit",
        include_str!("templates/shared/vfs-batch-edit.md.j2"),
    );
    b.register_partial(
        "shared/vfs-refactoring",
        include_str!("templates/shared/vfs-refactoring.md.j2"),
    );
    b.register_partial(
        "shared/vfs-analysis-table",
        include_str!("templates/shared/vfs-analysis-table.md.j2"),
    );
    b.register_partial(
        "shared/vfs-symbol-naming",
        include_str!("templates/shared/vfs-symbol-naming.md.j2"),
    );
    b.register_partial(
        "shared/vfs-agent-discipline",
        include_str!("templates/shared/vfs-agent-discipline.md.j2"),
    );
    b.register_partial("shared/vfs-full", include_str!("templates/shared/vfs-full.md.j2"));
}
/// Create a [`HandleBuilder`] with VFS globals, macros, and shared
/// content partials pre-registered.
///
/// Single entry point for both the provider-side template engine and
/// each hook-side template engine — every caller gets the same shared
/// partials without repeating the two-line preamble.
pub fn new_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    register_shared_partials(&mut b);
    b
}
