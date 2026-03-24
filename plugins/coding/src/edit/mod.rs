//! File editing operations -- edit planning, content splicing, and slice parsing.

/// Shared trait for diff-preview-then-apply-on-delete patterns.
pub mod diff_action;

/// Edit planning and resolution for multi-operation file edits.
pub mod plan;

/// Content splicing with validation.
pub mod splice;
