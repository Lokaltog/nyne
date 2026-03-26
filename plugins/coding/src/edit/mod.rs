//! File editing operations -- edit planning, content splicing, and slice parsing.

//! Batch edit staging, splice application, and diff-based code actions.
//!
//! This module implements the write side of the VFS: agents stage edits
//! (replace, insert-before/after, append, delete) against decomposed symbols,
//! preview them as unified diffs, and apply them atomically with tree-sitter
//! validation.

/// The [`DiffAction`] trait for nodes that preview as a diff and apply on delete.
pub mod diff_action;

/// Edit planning and resolution for multi-operation file edits.
pub mod plan;

/// Content splicing with validation.
pub mod splice;
