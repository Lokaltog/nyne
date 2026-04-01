//! Registry of named scripts indexed by dotted address.
//!
//! Built once at mount time during the plugin activation phase.
//! Each plugin contributes scripts via [`Plugin::scripts`](crate::plugin::Plugin::scripts);
//! they are indexed by their fully-qualified address (e.g., `provider.source.decompose`)
//! for O(1) lookup when `nyne exec` is invoked.
//!
//! Duplicate addresses are detected at registration time and logged as warnings --
//! the last registration wins, matching `HashMap` insert semantics.

use std::collections::HashMap;

use color_eyre::eyre::eyre;
use tracing::warn;

use super::script::{Script, ScriptAddress, ScriptContext};
use crate::prelude::*;

/// Registry of named scripts, indexed by fully-qualified dotted address.
///
/// Built once at mount time from pre-collected [`ScriptEntry`] values.
/// The registry is immutable after construction — scripts cannot be added
/// or removed at runtime.
///
/// Lookup is O(1) via `HashMap`. The `nyne exec` CLI command resolves an address
/// through this registry, then calls [`Script::exec`] with binary stdin/stdout.
pub struct ScriptRegistry {
    scripts: HashMap<ScriptAddress, Arc<dyn Script>>,
}

/// Script registration and execution.
impl ScriptRegistry {
    /// Build the registry from pre-collected script entries.
    ///
    /// Duplicate addresses are logged as warnings; the last registration wins
    /// (`HashMap` insert semantics).
    pub(crate) fn from_entries(entries: Vec<(ScriptAddress, Arc<dyn Script>)>) -> Self {
        let mut scripts = HashMap::new();
        for (address, script) in entries {
            if scripts.insert(address.clone(), script).is_some() {
                warn!(address = %address, "duplicate script address");
            }
        }
        Self { scripts }
    }

    /// Execute a script by its fully-qualified address.
    pub(crate) fn exec(&self, address: &str, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        self.scripts
            .get(address)
            .ok_or_else(|| eyre!("unknown script: {address}"))?
            .exec(ctx, stdin)
    }
}
