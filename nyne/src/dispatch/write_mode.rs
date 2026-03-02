/// How the FUSE write should be dispatched to the `Writable` capability.
///
/// Derived from the open flags (`O_TRUNC`, `O_APPEND`) stored on the file handle.
/// The pipeline passes this through so the final `Writable` method is chosen correctly.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Standard positional write (default).
    Normal,
    /// File was opened with `O_TRUNC` — full content replacement.
    Truncate,
}
