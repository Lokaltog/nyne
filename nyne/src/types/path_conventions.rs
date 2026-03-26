//! VFS path naming conventions — companion suffix, companion split, fragment parsing.

use crate::types::VfsPath;

/// Suffix appended to a filename to form its companion directory (`file.rs@`).
pub const COMPANION_SUFFIX: &str = "@";

/// Strip a suffix from a string, returning `None` if the suffix is absent
/// or if stripping it would leave an empty string.
///
/// The empty-string guard prevents a bare `@` from being treated as a valid
/// companion name (it has no associated real file).
fn strip_suffix_nonempty<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    s.strip_suffix(suffix).filter(|s| !s.is_empty())
}

/// Strip the companion suffix (`@`) from a directory name, returning the bare name.
///
/// Returns `None` when the input doesn't end with `@`, or when stripping it would
/// leave an empty string (bare `@`).
pub fn strip_companion_suffix(name: &str) -> Option<&str> { strip_suffix_nonempty(name, COMPANION_SUFFIX) }
/// Build the companion directory name for a base filename (e.g., `"lib.rs"` → `"lib.rs@"`).
pub fn companion_name(base: &str) -> String { format!("{base}{COMPANION_SUFFIX}") }

/// Result of splitting a path at its `@`-suffixed companion component.
///
/// Given `dir/file.rs@/symbols/Foo@`, the split produces:
/// - `source_file` = `dir/file.rs`
/// - `rest` = `["symbols", "Foo@"]` (raw, unstripped)
///
/// The lifetime borrows path segments from the input [`VfsPath`].
pub struct CompanionSplit<'a> {
    /// The real file that the companion directory is associated with.
    pub source_file: VfsPath,
    /// Path components after the `@`-suffixed entry, in order.
    /// These are **not** stripped of any `@` suffix — callers decide
    /// how to interpret them (e.g., syntax strips `@` from fragment dirs).
    pub rest: Vec<&'a str>,
}

/// Route dispatch helpers for companion path splits.
impl CompanionSplit<'_> {
    /// Rest segments as a `&str` slice for route dispatch.
    ///
    /// SSOT for the repeated pattern in `companion_children` and
    /// [`companion_lookup`](crate::providers::companion_lookup).
    pub fn rest_segments(&self) -> &[&str] { &self.rest }
}

/// Find the first `@`-suffixed component in `path` and split around it.
///
/// Returns `None` if no component ends with `@` or if
/// stripping the suffix would leave an empty name (bare `@`).
pub fn split_companion_path(path: &VfsPath) -> Option<CompanionSplit<'_>> {
    let mut parent_segments: Vec<&str> = Vec::new();

    let mut components = path.components();
    let real_name = loop {
        let comp = components.next()?;
        if let Some(name) = strip_companion_suffix(comp) {
            break name;
        }
        parent_segments.push(comp);
    };

    let mut source_file = VfsPath::root();
    for &seg in &parent_segments {
        source_file = source_file.join(seg).ok()?;
    }
    source_file = source_file.join(real_name).ok()?;

    Some(CompanionSplit {
        source_file,
        rest: components.collect(),
    })
}
