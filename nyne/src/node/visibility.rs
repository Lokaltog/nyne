/// Controls whether a node appears in directory listings.
///
/// Providers set this at construction time. The dispatch layer
/// checks it during readdir to filter entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Visible in readdir and accessible by name lookup (default).
    #[default]
    Readdir,
    /// Accessible by name lookup only — hidden from readdir listings.
    Hidden,
}
