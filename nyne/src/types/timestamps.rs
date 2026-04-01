use std::time::SystemTime;

/// Timestamp triplet for virtual filesystem nodes.
///
/// Separating timestamps into a struct avoids positional ambiguity —
/// each field is named rather than being the Nth `SystemTime` argument.
#[derive(Debug, Clone, Copy)]
pub struct Timestamps {
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
}

impl Timestamps {
    /// Create timestamps with all three fields set to the same value.
    ///
    /// Common case: inherit all timestamps from the source file's mtime.
    pub const fn uniform(time: SystemTime) -> Self {
        Self {
            atime: time,
            mtime: time,
            ctime: time,
        }
    }
}

impl Default for Timestamps {
    /// All fields default to `UNIX_EPOCH`.
    fn default() -> Self { Self::uniform(SystemTime::UNIX_EPOCH) }
}
