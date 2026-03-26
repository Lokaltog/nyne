//! Filesystem naming strategies and conflict resolution for fragments.

use std::collections::HashMap;

use nyne::text::slugify_unbounded;

use super::fragment::{ConflictSet, Fragment, FragmentKind, Resolution};

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

/// Assign `fs_name` to each fragment according to the given strategy.
/// Recurses into children.
pub fn apply_fs_mapping(fragments: &mut [Fragment], strategy: NamingStrategy) {
    match strategy {
        NamingStrategy::Identity => apply_identity(fragments),
        NamingStrategy::Slugified { indexed } => apply_slugified(fragments, indexed),
    }
}

/// Assign `fs_name` as the fragment's name verbatim (identity strategy).
fn apply_identity(fragments: &mut [Fragment]) {
    for frag in fragments {
        if !frag.kind.is_structural() {
            frag.fs_name = Some(frag.name.clone());
        }
        apply_identity(&mut frag.children);
    }
}

/// Assign `fs_name` as a kebab-case slug, optionally with a numeric prefix.
///
/// When `indexed` is true, names are prefixed with a zero-padded sequence
/// number (e.g. `00-getting-started`, `01-installation`) to preserve source
/// order in directory listings. Code blocks get a separate counter multiplied
/// by 10 to leave room for insertions.
fn apply_slugified(fragments: &mut [Fragment], indexed: bool) {
    let mut code_block_index = 0usize;
    for (i, frag) in fragments.iter_mut().enumerate() {
        apply_slugified(&mut frag.children, indexed);
        if frag.kind.is_structural() {
            continue;
        }
        if matches!(frag.kind, FragmentKind::CodeBlock { .. }) {
            code_block_index += 1;
            frag.fs_name = Some((code_block_index * 10).to_string());
        } else {
            let slug = slugify_unbounded(&frag.name);
            frag.fs_name = Some(if indexed { format!("{i:02}-{slug}") } else { slug });
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
