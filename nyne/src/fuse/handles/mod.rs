//! File handle management for buffered reads and writes.
//!
//! Each `open()` call allocates a [`HandleEntry`] in the slab-backed
//! [`HandleTable`], returning a file handle number (slab index) to FUSE.
//! Handles bridge stateless FUSE callbacks to stateful content buffering —
//! they track dirty state, truncation history, and copy-on-write content
//! via [`ContentBuffer`].
//!
//! Two distinct open paths exist:
//!
//! - **Buffered** ([`HandleTable::open`]) — content loaded into memory up
//!   front. Used for virtual files (provider-generated content).
//! - **Direct** ([`HandleTable::open_direct`]) — reads via `pread()` on a
//!   backing fd, with lazy buffer population on first write. Used for real
//!   files that benefit from kernel page cache avoidance.
//!
//! Both paths converge on flush: dirty buffers are flushed through the
//! router's write pipeline with a [`WriteMode`] derived from truncation
//! history (see [`HandleEntry::write_mode`]).

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::{io, mem};

use parking_lot::RwLock;
use slab::Slab;
use tracing::warn;

use crate::dispatch::WriteMode;

/// Unit tests.
#[cfg(test)]
mod tests;

/// Parsed open flags relevant to handle behavior.
///
/// Single source of truth for interpreting `O_TRUNC`, `O_APPEND`, and
/// write-intent from raw POSIX flags. All call sites use this instead
/// of inline bit checks.
pub(super) struct OpenMode {
    pub truncate: bool,
    pub append: bool,
    /// The file was opened with write intent (`O_WRONLY` or `O_RDWR`).
    pub write_intent: bool,
}

/// Methods for parsing POSIX open flags.
impl OpenMode {
    /// Parses raw POSIX open flags into structured mode flags.
    pub const fn parse(flags: i32) -> Self {
        let accmode = flags & libc::O_ACCMODE;
        Self {
            truncate: flags & libc::O_TRUNC != 0,
            append: flags & libc::O_APPEND != 0,
            write_intent: accmode == libc::O_WRONLY || accmode == libc::O_RDWR,
        }
    }
}

/// Source of truth for reads on a file handle.
///
/// Encodes the state machine that was previously implicit in the
/// combination of `direct_fd: Option<File>`, `truncated: bool`, and
/// `buffer.is_empty()`. Each variant makes the valid operations and
/// transitions explicit — invalid states are unrepresentable.
///
/// ```text
///   ┌────────────┐  write()   ┌──────────────┐
///   │  DirectFd  │──────────→ │ Materialized │
///   │ (pread I/O)│ populate   │ (buffer I/O) │
///   └────────────┘ from fd    └──────────────┘
///         │
///         │ truncate()
///         ▼
///   ┌────────────┐  write()   ┌──────────────┐
///   │  Truncated │──────────→ │ Materialized │
///   │ (empty buf)│ skip       │ (buffer I/O) │
///   └────────────┘ populate   └──────────────┘
///
///   ┌────────────┐
///   │ Preloaded  │  (buffer I/O from the start)
///   └────────────┘
/// ```
enum BufferSource {
    /// Content pre-loaded into buffer on open (virtual files, `O_TRUNC`
    /// real files). All reads and writes use the buffer.
    Preloaded,

    /// Reads via `pread()` on backing fd. Buffer is empty and has NOT
    /// been populated yet. On first write, the buffer is populated from
    /// the fd so non-overwritten regions retain their original data.
    DirectFd(File),

    /// Like `DirectFd`, but the file was truncated (`O_TRUNC` or
    /// `setattr`). The old content is logically gone — on first write,
    /// the buffer is NOT populated from the fd.
    Truncated(File),

    /// Transitioned from `DirectFd` or `Truncated` after the first
    /// write populated (or skipped populating) the buffer. The fd is
    /// kept alive for the handle's lifetime but no longer used for reads.
    ///
    /// `truncated` preserves whether the handle went through `Truncated`
    /// at any point — needed by [`WriteMode`] to choose between
    /// full-replacement and normal write semantics.
    Materialized { _fd: File, truncated: bool },
}

/// Methods for querying buffer source state.
impl BufferSource {
    /// Whether this source was truncated at some point in its lifecycle.
    /// Used to derive [`WriteMode`] for the flush pipeline.
    const fn was_truncated(&self) -> bool {
        matches!(self, Self::Truncated(_) | Self::Materialized { truncated: true, .. })
    }
}
/// Copy-on-write content buffer for file handles.
///
/// `Shared` holds an `Arc<[u8]>` from the content cache — reads
/// are zero-copy. On first write, the buffer materializes to `Owned`.
pub(super) enum ContentBuffer {
    /// Shared content from the content cache. Read-only until
    /// materialized by a write operation.
    Shared(Arc<[u8]>),
    /// Owned mutable buffer (after write, truncation, or direct-fd
    /// materialization).
    Owned(Vec<u8>),
}

impl ContentBuffer {
    /// Get the buffer contents as a byte slice.
    pub(super) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Shared(arc) => arc,
            Self::Owned(vec) => vec,
        }
    }

    /// Get a mutable reference to the buffer, materializing if shared.
    /// This is the COW trigger — called before any mutation.
    fn make_mut(&mut self) -> &mut Vec<u8> {
        if let Self::Shared(arc) = self {
            *self = Self::Owned(arc.to_vec());
        }
        match self {
            Self::Owned(vec) => vec,
            Self::Shared(_) => unreachable!(),
        }
    }

    fn len(&self) -> usize { self.as_bytes().len() }

    /// Clone the buffer contents to a `Vec<u8>`.
    fn to_vec(&self) -> Vec<u8> { self.as_bytes().to_vec() }
}

/// A file handle for FUSE read/write operations.
///
/// State machine with two axes:
/// - **Read source** ([`BufferSource`]): where reads come from (fd vs buffer)
/// - **Dirty generation** (`dirty_gen`): tracks writes for safe flush
pub(super) struct HandleEntry {
    /// The inode this handle is associated with.
    pub inode: u64,
    /// The buffered content (COW — shared until first mutation).
    pub buffer: ContentBuffer,
    /// Where reads come from and how writes populate the buffer.
    source: BufferSource,
    /// Dirty generation counter. Zero means clean. Each `write()` increments
    /// this. `clear_dirty` only resets to zero if the generation matches the
    /// snapshot — preventing a concurrent write from being silently lost.
    dirty_gen: u64,
    /// Whether the handle was opened with `O_APPEND`.
    ///
    /// With `FOPEN_DIRECT_IO`, the kernel positions `O_APPEND` writes at
    /// `i_size_read(inode)` rather than the buffer's actual length. If the
    /// kernel's cached `i_size` is stale (e.g., `getattr` returned a
    /// different size than `load_content`), the write offset overshoots and
    /// creates a null-byte gap. When this flag is set, `write()` clamps the
    /// offset to `buffer.len()` to guarantee appends land at the true EOF.
    append: bool,
    /// Whether this Preloaded handle discarded content at open time via
    /// `O_TRUNC`. Direct handles track truncation in [`BufferSource`]
    /// instead. Used by [`write_mode`](Self::write_mode) to select
    /// `WriteMode::Truncate` and by [`is_dirty`](Self::is_dirty) to
    /// ensure standalone truncation (`: > file`) triggers a flush.
    truncated_on_open: bool,
}

/// Methods for querying handle entry state.
impl HandleEntry {
    /// Returns whether the handle has uncommitted writes.
    pub const fn is_dirty(&self) -> bool { self.dirty_gen > 0 }

    /// Returns the write mode based on truncation history.
    pub const fn write_mode(&self) -> WriteMode {
        if self.truncated_on_open || self.source.was_truncated() {
            WriteMode::Truncate
        } else {
            // O_APPEND is handled at the buffer level: `write()` clamps
            // the offset to `buffer.len()`, so by flush time the buffer
            // already contains the final state. Passing Append to the
            // Writable would re-read the original content and append the
            // *entire* buffer (including the original content loaded on
            // open), doubling everything.
            WriteMode::Normal
        }
    }
}

/// File handle table using a slab for O(1) allocation and lookup.
///
/// File handle numbers are slab indices directly (no offset needed —
/// FUSE file handles have no reserved values).
pub(super) struct HandleTable {
    inner: RwLock<Slab<HandleEntry>>,
}

/// Default implementation for `HandleTable`.
impl Default for HandleTable {
    /// Creates a new, empty handle table.
    fn default() -> Self { Self::new() }
}

/// File handle operations for the FUSE filesystem.
impl HandleTable {
    /// Create a new, empty handle table.
    pub const fn new() -> Self {
        Self {
            inner: RwLock::new(Slab::new()),
        }
    }

    /// Open a buffered file: store initial content and open flags, return the file handle number.
    pub fn open(&self, inode: u64, content: Arc<[u8]>, open_flags: i32) -> u64 {
        let mode = OpenMode::parse(open_flags);
        // Truncating non-empty content is a mutation: mark dirty so the
        // empty buffer is flushed on release even without a subsequent write.
        // This makes standalone `: > virtualfile` actually clear the content.
        let content_was_nonempty = !content.is_empty();
        let buffer = if mode.truncate {
            ContentBuffer::Owned(Vec::new())
        } else {
            ContentBuffer::Shared(content)
        };
        let mut slab = self.inner.write();
        let idx = slab.insert(HandleEntry {
            inode,
            buffer,
            source: BufferSource::Preloaded,
            dirty_gen: u64::from(mode.truncate && content_was_nonempty),
            append: mode.append,
            truncated_on_open: mode.truncate,
        });
        idx as u64
    }

    /// Open a direct (fd-backed) file handle for a real file.
    ///
    /// Reads are served via `pread()` on the backing fd — no content is
    /// loaded into memory. Writes still use the buffer path (populated
    /// lazily on first write, flushed on release).
    pub fn open_direct(&self, inode: u64, file: File, open_flags: i32) -> u64 {
        let mode = OpenMode::parse(open_flags);
        let mut slab = self.inner.write();
        let source = if mode.truncate {
            BufferSource::Truncated(file)
        } else {
            BufferSource::DirectFd(file)
        };
        let idx = slab.insert(HandleEntry {
            inode,
            buffer: ContentBuffer::Owned(Vec::new()),
            source,
            dirty_gen: 0,
            append: mode.append,
            truncated_on_open: false,
        });
        idx as u64
    }

    /// Read from a file handle at the given offset.
    ///
    /// Dispatch depends on [`BufferSource`]:
    /// - `DirectFd`: reads via `pread()` on the backing fd (zero copy).
    /// - All others: reads from the in-memory buffer.
    ///
    /// Returns `Err` on IO failure (propagated to FUSE as `EIO`).
    pub fn read(&self, fh: u64, offset: u64, size: u32) -> io::Result<Vec<u8>> {
        let slab = self.inner.read();
        let Some(entry) = Self::get_entry(&slab, fh) else {
            return Ok(Vec::new());
        };

        // Direct fd path: pread() from the backing file.
        if let BufferSource::DirectFd(fd) = &entry.source {
            let size = usize::try_from(size).unwrap_or(usize::MAX);
            let mut buf = vec![0u8; size];
            let n = fd.read_at(&mut buf, offset)?;
            buf.truncate(n);
            return Ok(buf);
        }

        // Buffered path (Preloaded, Truncated, Materialized).
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let bytes = entry.buffer.as_bytes();
        if offset >= bytes.len() {
            return Ok(Vec::new());
        }
        let end = bytes.len().min(offset.saturating_add(size));
        Ok(bytes.get(offset..end).map(<[u8]>::to_vec).unwrap_or_default())
    }

    /// Write to a file handle's buffer at the given offset.
    ///
    /// For `DirectFd` handles, lazily populates the buffer from the backing
    /// fd — this ensures appends and random writes see the original file
    /// content instead of a zero-filled gap.
    ///
    /// For `Truncated` handles, skips population — the old content is gone.
    ///
    /// After the first write to any direct handle, the source transitions
    /// to `Materialized` and all subsequent reads use the buffer.
    ///
    /// Extends the buffer if the write goes past the end. Returns bytes written.
    pub fn write(&self, fh: u64, offset: u64, data: &[u8]) -> Option<u32> {
        let mut slab = self.inner.write();
        let entry = Self::get_entry_mut(&mut slab, fh)?;

        // On first write to a direct handle, transition to Materialized.
        // For DirectFd: populate buffer from fd first (preserve surrounding content).
        // For Truncated: skip population (old content is logically gone).
        let needs_populate = matches!(entry.source, BufferSource::DirectFd(_));
        let was_truncated = entry.source.was_truncated();
        if matches!(entry.source, BufferSource::DirectFd(_) | BufferSource::Truncated(_)) {
            let old = mem::replace(&mut entry.source, BufferSource::Preloaded);
            let (BufferSource::DirectFd(fd) | BufferSource::Truncated(fd)) = old else {
                unreachable!()
            };
            if needs_populate && let Err(e) = Self::populate_from_fd(&fd, entry.buffer.make_mut()) {
                warn!(target: "nyne::fuse", error = %e, "failed to populate buffer from direct fd");
                return None;
            }
            entry.source = BufferSource::Materialized {
                _fd: fd,
                truncated: was_truncated,
            };
        }

        // For O_APPEND handles, the kernel positions the write at
        // `i_size_read(inode)` which may be stale relative to the buffer.
        // Clamp to the actual buffer length to prevent null-byte gaps.
        let offset = if entry.append {
            entry.buffer.len()
        } else {
            usize::try_from(offset).unwrap_or(usize::MAX)
        };
        let end = offset.saturating_add(data.len());

        // Extend buffer if needed.
        let buf = entry.buffer.make_mut();
        if end > buf.len() {
            buf.resize(end, 0);
        }

        if let Some(slice) = buf.get_mut(offset..end) {
            slice.copy_from_slice(data);
        }
        if !data.is_empty() {
            entry.dirty_gen += 1;
        }
        u32::try_from(data.len()).ok()
    }

    /// Truncate a file handle's buffer to the given size.
    ///
    /// For `DirectFd` handles, transitions to `Truncated` so that a
    /// subsequent write does not re-read old content from the fd.
    /// If truncating to a non-zero size, the buffer is populated from the
    /// fd first so the leading bytes are preserved.
    ///
    /// For `Preloaded` handles that already have content in the buffer,
    /// marks the entry dirty so standalone truncation (`: > file`) is
    /// flushed on release. `DirectFd` handles don't need this — the
    /// `Truncated` source state already selects `WriteMode::Truncate`.
    pub fn truncate(&self, fh: u64, size: u64) {
        let mut slab = self.inner.write();
        if let Some(entry) = Self::get_entry_mut(&mut slab, fh) {
            Self::truncate_entry(entry, size);
        }
    }

    /// Truncate all handles for a given inode (for `setattr` without a file handle).
    ///
    /// Same semantics as [`truncate`](Self::truncate).
    pub fn truncate_by_inode(&self, inode: u64, size: u64) {
        let mut slab = self.inner.write();
        for (_, entry) in slab.iter_mut() {
            if entry.inode == inode {
                Self::truncate_entry(entry, size);
            }
        }
    }

    /// Shared truncation logic for a single entry.
    fn truncate_entry(entry: &mut HandleEntry, size: u64) {
        let size = usize::try_from(size).unwrap_or(usize::MAX);

        // DirectFd → Truncated: take the fd, optionally populate first.
        if matches!(entry.source, BufferSource::DirectFd(_)) {
            let old = mem::replace(&mut entry.source, BufferSource::Preloaded);
            let BufferSource::DirectFd(fd) = old else {
                unreachable!()
            };
            if size > 0
                && let Err(e) = Self::populate_from_fd(&fd, entry.buffer.make_mut())
            {
                warn!(target: "nyne::fuse", error = %e, "failed to populate buffer during truncate");
            }
            entry.source = BufferSource::Truncated(fd);
        } else if matches!(entry.source, BufferSource::Preloaded) && size < entry.buffer.len() {
            // Preloaded handle losing content — mark dirty so the
            // truncation is flushed even without a subsequent write.
            entry.dirty_gen += 1;
            entry.truncated_on_open = true;
        }

        entry.buffer.make_mut().truncate(size);
    }

    /// Check whether any open handle references the given inode.
    ///
    /// Used by the release path to decide whether per-inode state (write
    /// locks, error messages) can be cleaned up — cleanup is deferred as
    /// long as at least one handle remains open.
    pub fn has_handles_for_inode(&self, inode: u64) -> bool {
        let slab = self.inner.read();
        slab.iter().any(|(_, entry)| entry.inode == inode)
    }

    /// Get a snapshot of dirty buffer contents (for `flush` without releasing the handle).
    ///
    /// Returns `(buffer, write_mode, generation)` — the generation must be
    /// passed back to [`clear_dirty`](Self::clear_dirty) to prevent racing
    /// with concurrent writes.
    pub fn dirty_snapshot(&self, fh: u64) -> Option<(Vec<u8>, WriteMode, u64)> {
        let slab = self.inner.read();
        let entry = Self::get_entry(&slab, fh)?;
        (entry.dirty_gen > 0).then(|| (entry.buffer.to_vec(), entry.write_mode(), entry.dirty_gen))
    }

    /// Clear the dirty flag on a handle after a successful flush.
    ///
    /// Only clears if the current generation matches `snapshot_gen` — if a
    /// write arrived between `dirty_snapshot` and this call, the generation
    /// will have advanced and the dirty state is preserved, preventing data loss.
    pub fn clear_dirty(&self, fh: u64, snapshot_gen: u64) {
        let mut slab = self.inner.write();
        if let Some(entry) = Self::get_entry_mut(&mut slab, fh)
            && entry.dirty_gen == snapshot_gen
        {
            entry.dirty_gen = 0;
        }
    }

    /// Release a file handle, returning its entry if it existed.
    ///
    /// The caller should flush dirty buffers before or after this call.
    pub fn release(&self, fh: u64) -> Option<HandleEntry> {
        let idx = usize::try_from(fh).ok()?;
        let mut slab = self.inner.write();
        slab.contains(idx).then(|| slab.remove(idx))
    }

    /// Read the full contents of a backing fd into `buffer`.
    ///
    /// Used for lazy buffer population on first write to a direct fd handle.
    fn populate_from_fd(fd: &File, buffer: &mut Vec<u8>) -> io::Result<()> {
        let size = fd.metadata()?.len();
        if size == 0 {
            return Ok(());
        }
        let len = usize::try_from(size).unwrap_or(usize::MAX);
        buffer.clear();
        buffer.resize(len, 0);
        let n = fd.read_at(buffer, 0)?;
        buffer.truncate(n);
        Ok(())
    }

    /// Looks up a handle entry by file handle number.
    fn get_entry(slab: &Slab<HandleEntry>, fh: u64) -> Option<&HandleEntry> {
        let idx = usize::try_from(fh).ok()?;
        slab.get(idx)
    }

    /// Looks up a mutable handle entry by file handle number.
    fn get_entry_mut(slab: &mut Slab<HandleEntry>, fh: u64) -> Option<&mut HandleEntry> {
        let idx = usize::try_from(fh).ok()?;
        slab.get_mut(idx)
    }
}
