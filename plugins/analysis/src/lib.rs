//! nyne-analysis -- code analysis engine and rules for detecting code smells.
//!
//! This crate provides the analysis engine, rule trait, and built-in rules.
//! It is an optional dependency of the source and claude plugins.

pub mod config;
pub mod engine;

mod plugin;
mod providers;

pub use engine::{Engine, Hint, HintView};
pub use nyne_source::TsNode;
