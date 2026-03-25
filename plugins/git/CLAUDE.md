# nyne-plugin-git

Git plugin — blame, log, status, branches, diff, history, contributors, notes, git-aware companion renames. Module structure discoverable via VFS.

## Cross-Crate Consumers

nyne-plugin-coding depends on this crate for symbol-scoped git features (per-symbol blame/history). Public types are re-exported from `lib.rs` — consumers import from the crate root (e.g., `nyne_git::FileViewCtx`), never from internal module paths.

## Project Cloner

`clone.rs` implements `ProjectCloner` (trait defined in nyne core) for overlay lowerdir construction. Registered at link time via `#[distributed_slice(PROJECT_CLONERS)]` — core discovers it without any git-specific knowledge. Supports `Snapshot` (ODB copy of HEAD tree) and `Hardlink` (`git clone --local`) strategies.

## Graceful Degradation

If no git repo is found during `activate()`, no providers are created — the plugin silently disables itself.

## Branch Browsing

Branch names with `/` (e.g., `feat/lsp-diag-fix`) are decomposed into nested directories — FUSE dirent names cannot contain `/`. The `{..prefix}` rest-capture route handles arbitrary nesting depth. `children_branches_nested` does a two-phase dispatch: namespace prefix first, then longest-match branch name + tree path for file tree browsing via `GitRepo::ref_tree_entries`/`blob_at_ref`.

Leaf branch directories support `mv` (rename via `BranchRename`) and `rmdir` (delete via `BranchRemove`). Deletion only succeeds for branches fully merged into HEAD — unmerged or HEAD branches return `EACCES`.
