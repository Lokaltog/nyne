//! Tree-sitter-based source code decomposition and symbol extraction.
//!
//! This module provides language-aware decomposition of source files into
//! fragments (symbols, sections, code blocks). Each language is defined by a
//! [`LanguageSpec`](spec::LanguageSpec) that maps tree-sitter node kinds to
//! symbol types and provides parsing/formatting logic. Decomposers are
//! registered at compile time via the `register_syntax!` macro and indexed
//! by file extension in [`SyntaxRegistry`].

/// Shared decomposition cache and source file decomposition results.
pub mod decomposed;
/// Standard fragment extraction pipeline for trait-based language decomposers.
mod extract;
/// Fragment types representing decomposed pieces of a source file.
pub mod fragment;
/// Filesystem naming strategies and conflict resolution for fragments.
pub mod fs_mapping;
/// Injection-based compound decomposition for template languages.
mod injection;
/// Tree-sitter parsing utilities and code fragment construction.
pub mod parser;
/// Byte-range remapping for injection-based compound decomposition.
pub mod span_map;

/// Language specification trait and generic decomposer.
pub mod spec;
/// Minijinja template wrappers for decomposed source and fragments.
pub mod view;

/// Built-in language decomposers (Rust, Python, TypeScript, etc.).
pub mod languages;

use std::collections::HashMap;
use std::path::Path;

use color_eyre::eyre::eyre;
use nyne::path_utils::PathExt;
use nyne::prelude::*;
use spec::Decomposer;

/// Factory function that creates decomposer instances for link-time
/// auto-discovery via `linkme`.
///
/// Each factory returns `(extension, decomposer)` pairs. The `register_syntax!`
/// macro generates one factory per language type, but a single factory may
/// emit multiple extensions (e.g. TypeScript emits `"ts"`, `"tsx"`, `"js"`, `"jsx"`).
pub type SyntaxFactory = fn() -> Vec<(&'static str, Box<dyn Decomposer>)>;

/// Distributed slice collecting all registered syntax decomposer factories.
///
/// Populated at link time by `register_syntax!` invocations across language
/// modules. [`SyntaxRegistry::build`] iterates this slice to construct the
/// extension-indexed lookup table.
#[linkme::distributed_slice]
pub static SYNTAX_FACTORIES: [SyntaxFactory];

/// Register a language type as a syntax decomposer for its extensions.
///
/// # Examples
///
/// ```ignore
/// register_syntax!(RustLanguage);
/// ```
macro_rules! register_syntax {
    ($lang:ty) => {
        #[allow(unsafe_code)]
        #[linkme::distributed_slice($crate::syntax::SYNTAX_FACTORIES)]
        static _SYNTAX: $crate::syntax::SyntaxFactory = || {
            <$lang as $crate::syntax::spec::LanguageSpec>::EXTENSIONS
                .iter()
                .map(|&ext| {
                    let decomposer = Box::new($crate::syntax::spec::CodeDecomposer::<$lang>::new(ext))
                        as Box<dyn $crate::syntax::spec::Decomposer>;
                    (ext, decomposer)
                })
                .collect()
        };
    };
}
pub(crate) use register_syntax;

/// Generate a `map_symbol_kind` method body from declarative mappings.
///
/// Used inside `impl LanguageSpec` blocks to avoid hand-writing match arms.
///
/// # Examples
///
/// ```ignore
/// impl LanguageSpec for RustLanguage {
///     // ...
///     symbol_map! {
///         "function_item" => Function,
///         "struct_item"   => Struct,
///     }
/// }
/// ```
macro_rules! symbol_map {
    ($($node_kind:literal => $symbol:ident),* $(,)?) => {
        fn map_symbol_kind(node_kind: &str) -> Option<$crate::syntax::fragment::SymbolKind> {
            match node_kind {
                $($node_kind => Some($crate::syntax::fragment::SymbolKind::$symbol),)*
                _ => None,
            }
        }
    };
}
use std::sync::LazyLock;

pub(crate) use symbol_map;

/// Extension-indexed registry of syntax decomposers.
///
/// Built from the `linkme` distributed slice at startup. Provides O(1)
/// lookup by file extension. Also holds compound decomposers for
/// injection-based compound files (e.g. `.md.j2`).
pub struct SyntaxRegistry {
    decomposers: HashMap<&'static str, Arc<dyn Decomposer>>,
    /// Compound decomposers: `outer_ext → inner_ext → decomposer`.
    compound: HashMap<&'static str, HashMap<&'static str, Arc<dyn Decomposer>>>,
}

/// Lookup, construction, and symbol extraction methods for the registry.
impl SyntaxRegistry {
    /// Outer template extensions that support injection-based decomposition.
    const INJECTION_OUTERS: &[&'static str] = &["j2"];

    /// Build the registry from all registered syntax factories.
    #[must_use]
    pub fn build() -> Self {
        let mut decomposers = HashMap::new();
        for factory in SYNTAX_FACTORIES {
            for (ext, decomposer) in factory() {
                let arc: Arc<dyn Decomposer> = Arc::from(decomposer);
                assert!(
                    decomposers.insert(ext, arc).is_none(),
                    "duplicate syntax registration for extension: {ext}"
                );
            }
        }

        // Build compound (injection) decomposers for every registered inner
        // extension paired with each known outer template language.
        let mut compound: HashMap<&str, HashMap<&str, Arc<dyn Decomposer>>> = HashMap::new();
        for &outer in Self::INJECTION_OUTERS {
            let injections: HashMap<_, _> = decomposers
                .iter()
                .map(|(&ext, inner)| {
                    let decomposer: Arc<dyn Decomposer> =
                        Arc::new(injection::InjectionDecomposer::new(Arc::clone(inner), ext));
                    (ext, decomposer)
                })
                .collect();
            compound.insert(outer, injections);
        }

        Self { decomposers, compound }
    }

    /// Get the shared global registry instance.
    ///
    /// Built once on first access from the `linkme` distributed slice.
    /// All consumers share the same `Arc` — no duplicate builds.
    pub fn global() -> Arc<Self> {
        static INSTANCE: LazyLock<Arc<SyntaxRegistry>> = LazyLock::new(|| Arc::new(SyntaxRegistry::build()));
        Arc::clone(&INSTANCE)
    }

    /// Look up a decomposer by file extension (without the dot).
    #[must_use]
    pub fn get(&self, ext: &str) -> Option<&Arc<dyn Decomposer>> { self.decomposers.get(ext) }

    /// Look up a compound (injection) decomposer by inner and outer extensions.
    ///
    /// Zero-allocation: uses nested map lookup (`outer → inner → decomposer`).
    #[must_use]
    pub fn get_compound(&self, inner_ext: &str, outer_ext: &str) -> Option<&Arc<dyn Decomposer>> {
        self.compound.get(outer_ext)?.get(inner_ext)
    }

    /// Look up the right decomposer for a file path.
    ///
    /// Tries compound extension first (e.g. `.md.j2` → inner=`md`, outer=`j2`),
    /// then falls back to simple extension lookup. This is the **single source
    /// of truth** for "given a path, which decomposer handles it?" — all call
    /// sites must use this rather than calling `get`/`get_compound` directly.
    #[must_use]
    pub fn decomposer_for(&self, path: &Path) -> Option<&Arc<dyn Decomposer>> {
        if let Some((inner, outer)) = path.compound_extension()
            && let Some(d) = self.get_compound(inner, outer)
        {
            return Some(d);
        }
        let ext = path.extension()?.to_str()?;
        self.get(ext)
    }

    /// Return all registered extensions.
    #[must_use]
    pub fn extensions(&self) -> Vec<&'static str> {
        let mut exts: Vec<_> = self.decomposers.keys().copied().collect();
        exts.sort_unstable();
        exts
    }

    /// Extract a symbol body from source text by decomposing and navigating the
    /// fragment tree by `fs_name` components.
    ///
    /// Returns `None` if no decomposer exists for the extension, the source is
    /// not valid UTF-8, or the fragment path doesn't match any symbol.
    #[must_use]
    pub fn extract_symbol(
        &self,
        source: &str,
        ext: &str,
        fragment_path: &[String],
        max_depth: usize,
    ) -> Option<String> {
        let decomposer = self.get(ext)?;
        let (mut fragments, _tree) = decomposer.decompose(source, max_depth);
        decomposer.map_to_fs(&mut fragments);
        resolve_conflicts(&mut fragments, decomposer);
        let frag = find_fragment(&fragments, fragment_path)?;
        Some(source[frag.byte_range.start..frag.byte_range.end].to_owned())
    }
}

/// Detect and resolve `fs_name` conflicts at each level of a fragment tree.
///
/// After [`Decomposer::decompose`] and [`Decomposer::map_to_fs`], sibling
/// fragments may share the same `fs_name`. This function groups them into
/// [`ConflictSet`](fragment::ConflictSet)s and delegates resolution to [`Decomposer::resolve_conflicts`]
/// (which typically appends `~Kind` suffixes).
pub fn resolve_conflicts(fragments: &mut [fragment::Fragment], decomposer: &Arc<dyn Decomposer>) {
    let mut name_indices: HashMap<&str, Vec<usize>> = HashMap::with_capacity(fragments.len());
    for (i, frag) in fragments.iter().enumerate() {
        if let Some(fs_name) = &frag.fs_name {
            name_indices.entry(fs_name.as_str()).or_default().push(i);
        }
    }

    let conflicts: Vec<_> = name_indices
        .into_iter()
        .filter(|(_, indices)| indices.len() > 1)
        .map(|(name, indices)| fragment::ConflictSet {
            name: name.to_owned(),
            entries: indices
                .iter()
                .filter_map(|&i| {
                    let frag = fragments.get(i)?;
                    Some(fragment::ConflictEntry {
                        index: i,
                        fragment_name: frag.name.clone(),
                        fragment_kind: frag.kind.clone(),
                    })
                })
                .collect(),
        })
        .collect();

    if !conflicts.is_empty() {
        for res in decomposer.resolve_conflicts(&conflicts) {
            if let Some(frag) = fragments.get_mut(res.index) {
                frag.fs_name = res.fs_name;
            }
        }
    }

    // Recurse into children.
    for frag in fragments.iter_mut() {
        if !frag.children.is_empty() {
            resolve_conflicts(&mut frag.children, decomposer);
        }
    }
}

/// Find the deepest fragment whose `line_range` contains `line` (0-based)
/// and return its `fs_name` path segments.
///
/// Walks children recursively, returning the most specific match.
/// Nameless fragments (e.g. inherent impl blocks hidden by conflict
/// resolution) are transparent — the search looks through to their children.
/// Structural fragments (docstrings, imports, decorators) are skipped entirely.
/// Used by LSP symlink directories to reverse-map locations to symbols.
pub fn find_fragment_at_line(fragments: &[fragment::Fragment], line: usize, source: &str) -> Option<Vec<String>> {
    let rope = crop::Rope::from(source);
    find_fragment_at_line_rope(fragments, line, &rope)
}

/// Join fragment path segments into a VFS display path.
///
/// E.g., `&["Foo", "bar"]` → `"Foo@/bar"`. Uses the companion's runtime
/// suffix to build the path separator.
#[cfg(test)]
pub fn fragment_vfs_name(companion: &nyne_companion::Companion, segments: &[impl AsRef<str>]) -> String {
    let sep = format!("{}/", companion.companion_name(""));
    segments.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(&sep)
}

/// Inner implementation that accepts a pre-built rope to avoid repeated construction.
fn find_fragment_at_line_rope(fragments: &[fragment::Fragment], line: usize, rope: &crop::Rope) -> Option<Vec<String>> {
    let frag = fragments
        .iter()
        .filter(|f| !f.kind.is_structural())
        .find(|f| f.line_range(rope).contains(&line))?;

    let Some(fs_name) = frag.fs_name.as_ref() else {
        // Nameless container: look through to children.
        return find_fragment_at_line_rope(&frag.children, line, rope);
    };

    let mut path = find_fragment_at_line_rope(&frag.children, line, rope).unwrap_or_default();
    path.insert(0, fs_name.clone());
    Some(path)
}

/// Like [`find_fragment_at_line`], but falls back to the nearest fragment
/// when `line` is in a gap (imports, blank lines between items).
///
/// Proximity is measured from the line to the nearest range boundary.
/// Preceding fragments (whose range ends before `line`) are preferred
/// over following fragments at equal distance.
///
/// Nameless fragments (e.g. inherent impl blocks hidden by conflict
/// resolution) are transparent — when `line` falls inside one, the search
/// narrows to its children.
pub fn find_nearest_fragment_at_line(
    fragments: &[fragment::Fragment],
    line: usize,
    source: &str,
) -> Option<Vec<String>> {
    let rope = crop::Rope::from(source);
    find_nearest_fragment_at_line_rope(fragments, line, &rope)
}

/// Inner implementation that accepts a pre-built rope to avoid repeated construction.
fn find_nearest_fragment_at_line_rope(
    fragments: &[fragment::Fragment],
    line: usize,
    rope: &crop::Rope,
) -> Option<Vec<String>> {
    // If line falls inside a nameless container (e.g. hidden impl block),
    // narrow search to its children. Structural fragments (docstrings,
    // imports, decorators) are skipped — they're metadata, not containers.
    if let Some(frag) = fragments
        .iter()
        .filter(|f| !f.kind.is_structural())
        .find(|f| f.line_range(rope).contains(&line) && f.fs_name.is_none())
    {
        return find_nearest_fragment_at_line_rope(&frag.children, line, rope);
    }

    // Fast path: exact match.
    if let Some(path) = find_fragment_at_line_rope(fragments, line, rope) {
        return Some(path);
    }

    // Fallback: find the fragment with the closest range boundary.
    let (_, nearest) = fragments
        .iter()
        .filter(|f| f.fs_name.is_some())
        .map(|f| {
            // Distance to nearest boundary of this fragment's line range.
            let lr = f.line_range(rope);
            let dist = if line < lr.start {
                lr.start - line
            } else {
                // line >= lr.end (since contains() failed above)
                line - (lr.end.saturating_sub(1))
            };
            // Prefer preceding fragments (end ≤ line) at equal distance
            // by making following fragments sort after.
            let precedes = lr.end <= line;
            let key = (dist, !precedes);
            (key, f)
        })
        .min_by_key(|(key, _)| *key)?;

    let fs_name = nearest.fs_name.as_ref()?;
    let path = vec![fs_name.clone()];

    // Don't recurse into children — the line is outside this fragment's range,
    // so child ranges (subsets of the parent) won't contain it either.
    Some(path)
}

/// Navigate a fragment tree by following `fs_name` components.
///
/// Walks down the tree matching each path segment against the `fs_name` of
/// sibling fragments. Returns `None` if any segment has no match.
/// Used by VFS path resolution to map filesystem lookups to specific symbols.
pub fn find_fragment<'a>(fragments: &'a [fragment::Fragment], path: &[String]) -> Option<&'a fragment::Fragment> {
    let (first, rest) = path.split_first()?;
    let frag = fragments
        .iter()
        .find(|f| f.fs_name.as_deref() == Some(first.as_str()))?;
    if rest.is_empty() {
        Some(frag)
    } else {
        find_fragment(&frag.children, rest)
    }
}

/// Like [`find_fragment`], but returns an error if the fragment is not found.
///
/// Convenience wrapper for call sites that need `Result` instead of `Option`.
pub fn require_fragment<'a>(fragments: &'a [fragment::Fragment], path: &[String]) -> Result<&'a fragment::Fragment> {
    find_fragment(fragments, path).ok_or_else(|| eyre!("fragment not found: {}", path.join("/")))
}

/// Tests for syntax decomposition.
#[cfg(test)]
mod tests;
