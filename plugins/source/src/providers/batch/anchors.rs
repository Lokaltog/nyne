//! Anchor resolution for mapping filesystem write operations to staged edit actions.
//!
//! When an agent writes to a file like `file.rs@/symbols/Foo@/edit/replace`,
//! the filesystem operation name (`replace`, `delete`, `insert-before`, etc.)
//! is the *anchor*. This module validates the anchor against the current
//! fragment tree and produces a [`StagedAction`] ready for the staging area.

use std::io;
use std::str::from_utf8;

use color_eyre::eyre::Result;
use nyne::err::io_err;

use super::staging::StagedAction;
use crate::edit::plan::{EditOp, EditOpKind};
use crate::syntax::find_fragment;
use crate::syntax::fragment::{Fragment, FragmentKind};

/// Map a filesystem anchor operation to a [`StagedAction`].
///
/// Validates that the target symbol exists in the fragment tree and that
/// the anchor kind is appropriate for the target. Specifically, `Append`
/// is only allowed on scope symbols (impl blocks, modules, etc.) or
/// fragments with children — leaf symbols must use `InsertAfter` instead.
///
/// # Errors
///
/// Returns `NotFound` if `fragment_path` does not resolve to a known
/// fragment, or `InvalidInput` if `Append` is used on a non-scope symbol.
/// Also errors if `content` is not valid UTF-8 (for operations that carry
/// content).
pub(super) fn resolve_anchor(
    kind: EditOpKind,
    fragment_path: &[String],
    content: &[u8],
    fragments: &[Fragment],
) -> Result<StagedAction> {
    let target_name = fragment_path.last().map_or("(root)", String::as_str);
    let frag = find_fragment(fragments, fragment_path).ok_or_else(|| {
        io_err(
            io::ErrorKind::NotFound,
            format!("symbol '{target_name}' not found in fragment tree"),
        )
    })?;

    // Append is only valid on scope symbols (impl blocks, modules) or
    // fragments with children.
    if kind == EditOpKind::Append {
        let is_scope = matches!(&frag.kind, FragmentKind::Symbol(k) if k.is_scope()) || !frag.children.is_empty();
        if !is_scope {
            return Err(io_err(
                io::ErrorKind::InvalidInput,
                format!("'{target_name}' is not a scope — use insert_after instead"),
            ));
        }
    }

    let content = if kind == EditOpKind::Delete {
        None
    } else {
        Some(
            from_utf8(content)
                .map(String::from)
                .map_err(|e| color_eyre::eyre::eyre!("content is not valid UTF-8: {e}"))?,
        )
    };

    let op = EditOp {
        fragment_path: fragment_path.to_owned(),
        kind,
        content,
    };

    Ok(StagedAction { op })
}
