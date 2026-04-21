//! FUSE protocol macros used by the callback handlers.
//!
//! Every FUSE callback that can fail has to `reply.error(errno)` and
//! `return;` on the error path — if the handler panics or drops the
//! reply without calling a method on it, the kernel blocks. These
//! macros centralise the "log + reply + return" dance so the callback
//! bodies stay focused on the successful path.
//!
//! Consumers under `fuse/` import via `super::{fuse_err, fuse_try, ...}`
//! thanks to the `pub(super) use` re-exports at the bottom of this
//! file.
//!
//! All macros assume:
//! - `debug!` is in scope (`tracing::debug`).
//! - For `ensure_dir_path!`/`prepare_mutation!`, the expression passed
//!   as `$self` has `.inodes` exposing a `dir_path_for(u64)` lookup.
//! - For `fuse_try!`, `extract_errno` is reachable via
//!   `$crate::err::extract_errno`.
//! - The macros emit `return;`, so they are only valid inside the body
//!   of a FUSE callback that returns `()`.

/// Evaluate a fallible expression; on error, log + reply with the mapped errno.
macro_rules! fuse_try {
    ($reply:expr, $expr:expr, $ino:expr, $msg:literal) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                let errno = $crate::err::extract_errno(&e);
                debug!(target: "nyne::fuse", ino = $ino, error = %e, errno = ?errno, $msg);
                $reply.error(errno);
                return;
            }
        }
    };
}

/// Reply with a statically-known errno, logging at debug level.
///
/// Unlike [`fuse_try!`] (which extracts an errno from an error chain),
/// this is for deterministic validation failures where the errno is
/// known at the call site. Emits a `debug!` event so all FUSE error
/// replies show up in the same `nyne::fuse` tracing target.
macro_rules! fuse_err {
    ($reply:expr, $errno:expr, $ino:expr, $msg:literal) => {{
        let errno = $errno;
        debug!(target: "nyne::fuse", ino = $ino, errno = ?errno, $msg);
        $reply.error(errno);
        return;
    }};
}

/// Resolve a parent inode to its directory path, or reply ENOENT.
macro_rules! ensure_dir_path {
    ($self:expr, $parent:expr, $reply:expr) => {
        match $self.inodes.dir_path_for($parent) {
            Some(path) => path,
            None => {
                $reply.error(Errno::ENOENT);
                return;
            }
        }
    };
}

/// Resolve the (parent, name) addressing for a mutation and emit its debug log.
///
/// Used by `do_create`, `do_mkdir`, and `do_remove` to factor out the
/// repeated `u64::from(parent)` + `ensure_dir_path!` + `to_string_lossy` +
/// `debug!` sequence. Returns the numeric parent inode, the parent's
/// directory path, and the entry name as a `Cow<'_, str>`. Callers that
/// also need the full child path compute it via `dir_path.join(name)`.
macro_rules! prepare_mutation {
    ($self:expr, $parent:expr, $name:expr, $reply:expr, $op:literal) => {{
        let parent = u64::from($parent);
        let dir_path = ensure_dir_path!($self, parent, $reply);
        let name = $name.to_string_lossy();
        debug!(target: "nyne::fuse", parent, name = %name, $op);
        (parent, dir_path, name)
    }};
}

/// Reply with ENOTSUP, logging at debug level. Use for stub FUSE handlers
/// that intentionally reject the operation (e.g. `mknod`, `symlink`, `link`).
/// Inode field logged as `0` since these handlers don't address a live inode.
macro_rules! reply_enotsup {
    ($reply:expr, $op:literal) => {
        fuse_err!($reply, Errno::from_i32(libc::ENOTSUP), 0u64, $op)
    };
}

pub(crate) use ensure_dir_path;
pub(crate) use fuse_err;
pub(crate) use fuse_try;
pub(crate) use prepare_mutation;
pub(crate) use reply_enotsup;
