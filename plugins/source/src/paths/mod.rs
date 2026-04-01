use crate::plugin::config::vfs::VfsDirs;

/// VFS path builder for source plugin paths.
///
/// Inserted into [`ActivationContext`] by the source plugin during `activate()`.
/// Downstream plugins that depend on source use this to construct paths within
/// source's namespace without knowing the configuration internals.
///
/// [`ActivationContext`]: nyne::dispatch::activation::ActivationContext
#[derive(Debug)]
pub struct SourcePaths {
    symbols: String,
    at_line: String,
}

impl Default for SourcePaths {
    fn default() -> Self { Self::from_vfs(&VfsDirs::default()) }
}

impl SourcePaths {
    /// Construct with explicit directory names.
    pub fn new(symbols: impl Into<String>, at_line: impl Into<String>) -> Self {
        Self {
            symbols: symbols.into(),
            at_line: at_line.into(),
        }
    }

    /// Construct from the source plugin's resolved VFS directory config.
    pub(crate) fn from_vfs(dirs: &VfsDirs) -> Self { Self::new(dirs.symbols.clone(), dirs.at_line.clone()) }

    /// The top-level symbols directory name (e.g. `"symbols"`).
    pub fn symbols_dir(&self) -> &str { &self.symbols }

    /// Path to the at-line lookup for a specific line.
    ///
    /// Returns `"symbols/at-line/{line}"` (using configured dir names).
    pub fn at_line(&self, line: usize) -> String { format!("{}/{}/{line}", self.symbols, self.at_line) }
}

#[cfg(test)]
mod tests;
