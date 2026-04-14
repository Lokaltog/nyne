//! Test helpers for nyne-source.

use crate::syntax::SyntaxRegistry;
use crate::syntax::fragment::DecomposedFile;

/// Build a `SyntaxRegistry` with all compiled-in languages.
///
/// Shorthand for [`SyntaxRegistry::build()`] so tests don't need to import
/// the registry type directly.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Decompose `source` with the language registered for `ext` and return the
/// fragment tree. Panics if no language is registered for `ext`.
///
/// Shared helper used by per-language `basic()` fixtures.
pub fn decompose_fixture(ext: &str, source: &str) -> DecomposedFile {
    let (result, _tree) = registry().get(ext).unwrap().decompose(source, 5);
    result
}
