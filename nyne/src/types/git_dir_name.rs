/// VFS-relative name of the git metadata directory (usually `.git`).
///
/// Inserted into the [`TypeMap`](super::TypeMap) by the git plugin during
/// activation. Core infrastructure (path filter, watcher) reads this to
/// exclude the git directory from VFS listings and inotify watches.
///
/// Core never imports the git plugin — it only reads this core-defined type.
#[derive(Debug, Clone)]
pub struct GitDirName(pub Option<String>);
