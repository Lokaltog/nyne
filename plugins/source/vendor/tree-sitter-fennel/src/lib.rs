//! Fennel grammar for tree-sitter.
//!
//! C grammar source is fetched at build time from
//! <https://github.com/alexmozaidze/tree-sitter-fennel>.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_fennel() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Fennel.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_fennel) };
