/// Diff middleware — unified preview-and-apply handling for `.diff` virtual files.
///
/// Providers set [`DiffCapable`](provider::DiffCapable) request state to declare
/// that a path produces a diff. This middleware consumes that state:
/// - **Lookup**: creates a file node with a unified diff preview as `Readable`.
/// - **Remove**: computes and applies the edits to source files on disk.
pub(crate) mod provider;

pub use provider::{
    DiffCapable, DiffRequest, DiffSource, DiffUnlinkable, EditOutcome, FileEditResult, ValidationResult,
    apply_file_edits,
};

mod plugin;
