//! Filesystem naming strategies and conflict resolution for fragments.
//!
//! After decomposition, each fragment has a logical `name` (e.g. `"Foo"`,
//! `"Getting Started"`) but no filesystem-safe identifier. This module
//! assigns `fs_name` values via a [`NamingStrategy`] (identity for code,
//! slugified for documents) and then resolves collisions via a
//! [`ConflictStrategy`] (`~Kind` suffixes for code, numeric suffixes for
//! documents). The `fs_name` is what appears as the directory name in the
//! VFS `symbols/` namespace.

use std::collections::HashMap;

use nyne::text::slugify_unbounded;

use super::fragment::{ConflictEntry, ConflictSet, Fragment, FragmentKind, Resolution};

/// Separator used for `~Kind` disambiguation in fragment filesystem names
/// (e.g., `Foo~Struct`, `Foo~Impl`).
pub const DISAMBIGUATOR_SEP: char = '~';

/// Split a `~Kind`-disambiguated filesystem name into the bare symbol name
/// and the optional kind suffix.
///
/// # Examples
///
/// - `"Foo~Struct"` → `("Foo", Some("Struct"))`
/// - `"Foo"` → `("Foo", None)`
/// - `"Display_for_Foo~Impl"` → `("Display_for_Foo", Some("Impl"))`
pub fn split_disambiguator(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once(DISAMBIGUATOR_SEP) {
        Some((base, kind)) if !base.is_empty() && !kind.is_empty() => (base, Some(kind)),
        _ => (name, None),
    }
}

/// Strategy for assigning filesystem names to fragments.
///
/// Chosen by each language's `Decomposer::map_to_fs` implementation.
/// Code languages use `Identity` (symbol names are already unique-ish),
/// while document languages use `Slugified` (headings need kebab-casing
/// and sometimes index prefixes for ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingStrategy {
    /// Fragment name used as-is (default for code languages).
    Identity,
    /// Kebab-case slug with optional zero-padded index prefix (e.g. `00-getting-started`).
    Slugified { indexed: bool },
}

/// Strategy for resolving filesystem name collisions among sibling fragments.
///
/// Applied after naming, only when two or more siblings share the same
/// `fs_name`. Code languages use `KindSuffix` (preserves the symbol name,
/// appends the kind), document languages use `Numbered` (appends `-2`, `-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Append `~Kind` suffix (e.g. `foo~Function`, `foo~Struct`).
    /// If still ambiguous after kind suffix, hide all but the first.
    KindSuffix,
    /// First entry keeps its name, subsequent get `-2`, `-3`, etc.
    Numbered,
}

/// Run the naming and conflict-resolution pipeline over a fragment tree.
///
/// Performs a single recursive walk: descends into children first, then
/// assigns `fs_name` to siblings at the current level via [`NamingStrategy`],
/// then resolves any collisions among those siblings via [`ConflictStrategy`].
/// This is the single entry point — callers no longer need to invoke naming
/// and conflict resolution separately.
pub fn assign_fs_names(fragments: &mut [Fragment], naming: NamingStrategy, conflict: ConflictStrategy) {
    for frag in fragments.iter_mut() {
        assign_fs_names(&mut frag.children, naming, conflict);
    }
    apply_naming(fragments, naming);
    resolve_level_conflicts(fragments, conflict);
}

/// Assign `fs_name` to every non-structural fragment in `fragments` according
/// to `strategy`. Operates on a single sibling level — recursion is owned by
/// [`assign_fs_names`].
fn apply_naming(fragments: &mut [Fragment], strategy: NamingStrategy) {
    match strategy {
        NamingStrategy::Identity => apply_identity(fragments),
        NamingStrategy::Slugified { indexed } => apply_slugified(fragments, indexed),
    }
}

/// Identity naming: `fs_name` is the fragment's `name` verbatim.
fn apply_identity(fragments: &mut [Fragment]) {
    for frag in fragments.iter_mut() {
        if !frag.kind.is_structural() {
            frag.fs_name = Some(frag.name.clone());
        }
    }
}

/// Slugified naming: kebab-case slug, optionally with a zero-padded source
/// order prefix. Code blocks use a separate counter (multiplied by 10 to
/// leave insertion gaps) since their slug would just be empty.
fn apply_slugified(fragments: &mut [Fragment], indexed: bool) {
    let mut code_block_index = 0usize;
    for (i, frag) in fragments.iter_mut().enumerate() {
        if frag.kind.is_structural() {
            continue;
        }
        frag.fs_name = Some(slugified_fs_name(frag, i, indexed, &mut code_block_index));
    }
}

/// Compute the slugified `fs_name` for a single fragment. Extracted so
/// [`apply_slugified`] doesn't carry deeply nested branches.
fn slugified_fs_name(frag: &Fragment, sibling_index: usize, indexed: bool, code_block_index: &mut usize) -> String {
    if matches!(frag.kind, FragmentKind::CodeBlock { .. }) {
        *code_block_index += 1;
        return (*code_block_index * 10).to_string();
    }
    let slug = slugify_unbounded(&frag.name);
    if indexed {
        format!("{sibling_index:02}-{slug}")
    } else {
        slug
    }
}

/// Detect `fs_name` collisions among `fragments` (siblings) and apply
/// `strategy`'s resolution. Operates on a single sibling level — recursion
/// is owned by [`assign_fs_names`].
fn resolve_level_conflicts(fragments: &mut [Fragment], strategy: ConflictStrategy) {
    let mut name_indices: HashMap<&str, Vec<usize>> = HashMap::with_capacity(fragments.len());
    for (i, frag) in fragments.iter().enumerate() {
        if let Some(fs_name) = &frag.fs_name {
            name_indices.entry(fs_name.as_str()).or_default().push(i);
        }
    }

    let conflicts: Vec<ConflictSet> = name_indices
        .into_iter()
        .filter(|(_, indices)| indices.len() > 1)
        .map(|(name, indices)| ConflictSet {
            name: name.to_owned(),
            entries: indices
                .into_iter()
                .filter_map(|i| {
                    fragments.get(i).map(|frag| ConflictEntry {
                        index: i,
                        fragment_kind: frag.kind.clone(),
                    })
                })
                .collect(),
        })
        .collect();

    if conflicts.is_empty() {
        return;
    }

    for res in resolve_conflicts(&conflicts, strategy) {
        if let Some(frag) = fragments.get_mut(res.index) {
            frag.fs_name = res.fs_name;
        }
    }
}

/// Resolve filesystem name collisions according to the given strategy.
pub fn resolve_conflicts(conflicts: &[ConflictSet], strategy: ConflictStrategy) -> Vec<Resolution> {
    match strategy {
        ConflictStrategy::KindSuffix => resolve_kind_suffix(conflicts),
        ConflictStrategy::Numbered => resolve_numbered(conflicts),
    }
}

/// Resolve name collisions by appending `~Kind` suffixes (e.g. `Foo~Struct`, `Foo~Impl`).
///
/// When `~Kind` alone still produces duplicates (e.g. two `impl Foo` blocks →
/// `Foo~Impl`, `Foo~Impl`), the first occurrence keeps the bare `~Kind` name
/// and subsequent duplicates get a numeric suffix: `Foo~Impl`, `Foo~Impl-2`.
fn resolve_kind_suffix(conflicts: &[ConflictSet]) -> Vec<Resolution> {
    let sep = DISAMBIGUATOR_SEP;

    let mut resolutions = Vec::new();

    for conflict in conflicts {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for entry in &conflict.entries {
            let kind = entry.fragment_kind.to_string();
            let count = seen.entry(kind.clone()).or_insert(0);
            *count += 1;
            let fs_name = if *count == 1 {
                format!("{}{sep}{kind}", conflict.name)
            } else {
                format!("{}{sep}{kind}-{count}", conflict.name)
            };
            resolutions.push(Resolution {
                index: entry.index,
                fs_name: Some(fs_name),
            });
        }
    }

    resolutions
}

/// Resolve name collisions by appending numeric suffixes (e.g. `foo`, `foo-2`, `foo-3`).
///
/// The first occurrence keeps its bare name; subsequent occurrences in source
/// order get `-2`, `-3`, etc. Used by document languages where kind-based
/// disambiguation is not meaningful (all sections are the same kind).
fn resolve_numbered(conflicts: &[ConflictSet]) -> Vec<Resolution> {
    conflicts
        .iter()
        .flat_map(|conflict| {
            conflict.entries.iter().enumerate().map(move |(i, entry)| {
                let fs_name = if i == 0 {
                    conflict.name.clone()
                } else {
                    format!("{}-{}", conflict.name, i + 1)
                };
                Resolution {
                    index: entry.index,
                    fs_name: Some(fs_name),
                }
            })
        })
        .collect()
}

/// Tests for filesystem naming and conflict resolution.
#[cfg(test)]
mod tests;
