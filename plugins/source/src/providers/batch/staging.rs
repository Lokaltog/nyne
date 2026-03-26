//! Staging area state tracking for batch edits.
//!
//! Provides the data structures that back the in-memory staging area:
//! [`StagingKey`] identifies a specific symbol in a specific file,
//! [`StagedBatch`] holds the ordered list of [`StagedAction`]s for that
//! symbol. Actions are indexed with a step-based numbering scheme
//! ([`INDEX_STEP`]) that allows insertions without immediate renumbering.

use std::collections::BTreeMap;

use nyne::types::vfs_path::VfsPath;

use crate::edit::plan::{EditOp, EditPlan};

/// Key for staged edit lookup — identifies a specific symbol in a specific file.
///
/// Used as the `HashMap` key in [`StagingMap`](super::StagingMap) to group
/// staged actions by their target symbol. Two keys are equal when they refer
/// to the same fragment path in the same source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct StagingKey {
    /// The source file being edited.
    pub source_file: VfsPath,
    /// Fragment path identifying the target symbol (e.g., `["Foo"]` or `["Foo", "bar"]`).
    pub fragment_path: Vec<String>,
}

/// A single staged edit action within a [`StagedBatch`].
///
/// Wraps an [`EditOp`] which carries the operation kind (replace, delete,
/// insert-before, insert-after, append), target fragment path, and content.
/// Indexed by a `u32` key inside the parent batch's `BTreeMap`.
pub(super) struct StagedAction {
    /// The edit operation (SSOT for kind, content, and fragment path).
    pub op: EditOp,
}

/// Methods for [`StagedAction`].
impl StagedAction {
    /// Filename for this staged action (e.g., `10-replace.diff`).
    pub fn filename(&self, index: u32) -> String { format!("{:02}-{}.diff", index, self.op.kind.name()) }
}

/// Index increment between staged actions.
///
/// New actions are assigned indices at multiples of this step (10, 20, 30, ...),
/// leaving gaps for potential future reordering without immediate renumbering.
/// When a reorder operation exhausts the gap space, [`StagedBatch::renumber`]
/// reassigns contiguous indices.
const INDEX_STEP: u32 = 10;

/// All staged edits for a single symbol, keyed by [`StagingKey`].
///
/// Actions are stored in a `BTreeMap<u32, StagedAction>` for stable ordering.
/// New actions receive indices at [`INDEX_STEP`] increments (10, 20, 30, ...),
/// leaving gaps for reordering. When gap space is exhausted, [`renumber`](Self::renumber)
/// reassigns contiguous indices. Converted to an [`EditPlan`] for preview/apply.
pub(super) struct StagedBatch {
    /// Ordered list of staged actions, keyed by staged index.
    actions: BTreeMap<u32, StagedAction>,
    /// Next index to assign.
    next_index: u32,
}

#[allow(dead_code)] // reorder, renumber: intentional API surface for future use
/// Methods for [`StagedBatch`].
impl StagedBatch {
    /// Create an empty staged batch.
    pub const fn new() -> Self {
        Self {
            actions: BTreeMap::new(),
            next_index: INDEX_STEP,
        }
    }

    /// Add an action, returning the assigned index.
    pub fn stage(&mut self, action: StagedAction) -> u32 {
        let index = self.next_index;
        self.next_index += INDEX_STEP;
        self.actions.insert(index, action);
        index
    }

    /// Remove a staged action by index.
    pub fn remove(&mut self, index: u32) -> Option<StagedAction> { self.actions.remove(&index) }

    /// Move an action from `old_index` to `new_index`, then renumber all
    /// actions to consistent `INDEX_STEP` increments.
    pub fn reorder(&mut self, old_index: u32, new_index: u32) {
        let Some(action) = self.actions.remove(&old_index) else {
            return;
        };
        self.actions.insert(new_index, action);
        self.renumber();
    }

    /// Get a staged action by index.
    pub fn get(&self, index: u32) -> Option<&StagedAction> { self.actions.get(&index) }

    /// Update a staged action's content.
    pub fn update(&mut self, index: u32, content: String) {
        if let Some(action) = self.actions.get_mut(&index) {
            action.op.set_content(content);
        }
    }

    /// Convert staged actions to an `EditPlan` for preview/apply.
    pub fn to_edit_plan(&self) -> EditPlan {
        let ops: Vec<(u32, EditOp)> = self
            .actions
            .iter()
            .map(|(&idx, action)| (idx, action.op.clone()))
            .collect();
        EditPlan { ops }
    }

    /// Clear all staged actions.
    pub fn clear(&mut self) {
        self.actions.clear();
        self.next_index = INDEX_STEP;
    }

    /// Ordered iteration over staged actions.
    pub fn actions(&self) -> impl Iterator<Item = (u32, &StagedAction)> {
        self.actions.iter().map(|(&idx, action)| (idx, action))
    }

    /// Whether any actions are staged.
    pub fn is_empty(&self) -> bool { self.actions.is_empty() }

    /// Renumber all actions to consistent `INDEX_STEP` increments.
    #[allow(clippy::cast_possible_truncation)] // action counts are always small
    /// Reassigns contiguous index keys to all staged actions.
    fn renumber(&mut self) {
        use std::mem;
        let old = mem::take(&mut self.actions);
        for (i, (_, action)) in old.into_iter().enumerate() {
            let idx = (i as u32 + 1) * INDEX_STEP;
            self.actions.insert(idx, action);
        }
        self.next_index = (self.actions.len() as u32 + 1) * INDEX_STEP;
    }
}
