//! nyne-analysis -- code analysis engine and rules for detecting code smells.
//!
//! This crate provides the analysis engine, rule trait, and built-in rules.
//! It is an optional dependency of the source and claude plugins.

pub mod analysis;
pub mod config;

mod plugin;
mod providers;

pub use analysis::{Engine, Hint, HintView};
pub use nyne_source::TsNode;
