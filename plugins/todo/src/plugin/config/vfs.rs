nyne::vfs_struct! {
    /// VFS path configuration for the todo plugin.
    pub struct Vfs {
        /// Top-level todo directory name inside companion directories.
        todo = "todo",
        /// Overview file name inside the todo directory.
        overview = "OVERVIEW.md",
    }
}
