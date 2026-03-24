//! Filesystem naming strategies and conflict resolution for fragments.

use nyne::format::to_kebab;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingStrategy {
    /// Fragment name used as-is (default for code languages).
    Identity,
    /// Kebab-case slug with optional zero-padded index prefix (e.g. `00-getting-started`).
    Slugified { indexed: bool },
}

/// Strategy for resolving filesystem name collisions among sibling fragments.
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
            let slug = slugify(&frag.name);
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
fn resolve_kind_suffix(conflicts: &[ConflictSet]) -> Vec<Resolution> {
    let sep = DISAMBIGUATOR_SEP;

    let mut resolutions = Vec::new();

    for conflict in conflicts {
        // Try disambiguating by appending ~Kind.
        let disambiguated: Vec<String> = conflict
            .entries
            .iter()
            .map(|e| format!("{}{sep}{}", conflict.name, e.fragment_kind))
            .collect();

        // Check if all disambiguated names are unique.
        let unique = {
            let mut sorted = disambiguated.clone();
            sorted.sort();
            sorted.dedup();
            sorted.len() == disambiguated.len()
        };

        for (i, (disambig_name, entry)) in disambiguated.iter().zip(&conflict.entries).enumerate() {
            resolutions.push(Resolution {
                index: entry.index,
                fs_name: if unique {
                    Some(disambig_name.clone())
                } else {
                    // Can't disambiguate — hide all but the first.
                    (i == 0).then(|| conflict.name.clone())
                },
            });
        }
    }

    resolutions
}

/// Resolve name collisions by appending numeric suffixes (e.g. `foo`, `foo-2`, `foo-3`).
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

/// Convert a string to a filesystem-safe kebab-case slug.
///
/// Delegates to [`nyne::format::to_kebab`] with no length limit.
pub fn slugify(s: &str) -> String { to_kebab(s, usize::MAX) }

/// Tests for filesystem naming and conflict resolution.
#[cfg(test)]
mod tests;
