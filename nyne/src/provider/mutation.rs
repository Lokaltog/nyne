use crate::types::vfs_path::VfsPath;

/// A filesystem mutation operation on a real file.
///
/// Passed to [`Provider::handle_mutation`](super::Provider::handle_mutation)
/// so providers can intercept real-file mutations (e.g., update git index
/// on rename, stage deletions on unlink).
///
/// When no provider claims the operation (returns [`MutationOutcome::Handled`](super::MutationOutcome::Handled)),
/// the router falls back to the corresponding [`RealFs`](crate::types::real_fs::RealFs)
/// method. Either way, the actual filesystem change triggers inotify events
/// that flow through the watcher pipeline for cache invalidation.
#[derive(Debug)]
pub enum MutationOp<'a> {
    /// File or directory rename/move.
    Rename { from: &'a VfsPath, to: &'a VfsPath },
    /// File deletion (FUSE `unlink`).
    Unlink { path: &'a VfsPath },
    /// Directory removal (FUSE `rmdir`).
    Rmdir { path: &'a VfsPath },
    /// File creation (FUSE `create`).
    Create { path: &'a VfsPath },
    /// Directory creation (FUSE `mkdir`).
    Mkdir { path: &'a VfsPath },
}
