//! Provider implementations that depend on code analysis.

pub(crate) mod fragment_resolver;
pub(crate) mod names;
pub(crate) mod prelude;
pub(crate) mod util;

pub mod batch;
pub mod claude;
#[cfg(feature = "git-symbols")]
pub(crate) mod git_symbols_companion;
pub mod syntax;
pub mod todo;
