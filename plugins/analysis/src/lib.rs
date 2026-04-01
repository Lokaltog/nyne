//! nyne-analysis -- code analysis engine and rules for detecting code smells.
//!
//! This crate provides the analysis engine, rule trait, and built-in rules.
//! It is an optional dependency of the source and claude plugins.

pub(crate) mod context;
pub(crate) mod engine;

mod plugin;
mod provider;

pub use context::AnalysisContextExt;
pub use engine::{Engine, Hint, HintView};
pub(crate) use nyne_source::TsNode;
