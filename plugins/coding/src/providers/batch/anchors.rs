//! Anchor detection for line-based edits in batch operations.

use std::io;
use std::str::from_utf8;

use color_eyre::eyre::Result;

use super::staging::StagedAction;
use crate::edit::plan::{EditOp, EditOpKind};
use crate::syntax::find_fragment;
use crate::syntax::fragment::{Fragment, FragmentKind};

/// Map a filesystem anchor operation to a `StagedAction`.
///
/// Validates that `target_name` exists in the fragment tree and that
/// the anchor kind is appropriate for the target (e.g., `Append` only
/// on scope symbols).
pub(super) fn resolve_anchor(
    kind: EditOpKind,
    fragment_path: &[String],
    content: &[u8],
    fragments: &[Fragment],
) -> Result<StagedAction> {
    let target_name = fragment_path.last().map_or("(root)", String::as_str);
    let frag = find_fragment(fragments, fragment_path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("symbol '{target_name}' not found in fragment tree"),
        )
    })?;

    let content_str = || {
        from_utf8(content)
            .map(String::from)
            .map_err(|e| color_eyre::eyre::eyre!("content is not valid UTF-8: {e}"))
    };

    let op = match kind {
        EditOpKind::Replace => EditOp::Replace {
            fragment_path: fragment_path.to_owned(),
            content: content_str()?,
        },
        EditOpKind::Delete => EditOp::Delete {
            fragment_path: fragment_path.to_owned(),
        },
        EditOpKind::InsertBefore => EditOp::InsertBefore {
            fragment_path: fragment_path.to_owned(),
            content: content_str()?,
        },
        EditOpKind::InsertAfter => EditOp::InsertAfter {
            fragment_path: fragment_path.to_owned(),
            content: content_str()?,
        },
        EditOpKind::Append => {
            let is_scope =
                matches!(&frag.kind, FragmentKind::Symbol(kind) if kind.is_scope()) || !frag.children.is_empty();
            if !is_scope {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("'{target_name}' is not a scope — use insert_after instead"),
                )
                .into());
            }
            EditOp::Append {
                fragment_path: fragment_path.to_owned(),
                content: content_str()?,
            }
        }
    };

    Ok(StagedAction { op })
}
