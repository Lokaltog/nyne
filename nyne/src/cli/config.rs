//! `nyne config` — display the effective configuration.

use clap::Args;
use color_eyre::eyre::Result;

use super::output;
use crate::config::NyneConfig;
use crate::plugin::PLUGINS;

/// Arguments for the `config` subcommand.
///
/// Currently takes no arguments -- always displays the fully-resolved
/// configuration. Future flags (e.g., `--format`, `--section`) could be
/// added here without changing the dispatch interface.
#[derive(Debug, Args)]
pub struct ConfigArgs;

/// Display the effective configuration with all defaults and plugin configs resolved.
///
/// Loads the user's config file (or defaults), then iterates over all linked
/// plugins to replace their raw TOML tables with fully-resolved versions that
/// include plugin-specific defaults. This lets users see exactly what the
/// daemon will use, including values they never explicitly set.
///
/// Output is pretty-printed TOML written to stdout via [`output::term()`].
pub fn run(_args: &ConfigArgs) -> Result<()> {
    let mut config = NyneConfig::load()?;

    // Replace raw plugin tables with fully-resolved configs (defaults filled in).
    for factory in PLUGINS {
        let plugin = factory();
        if let Some(resolved) = plugin.resolved_config(&config) {
            config.plugin.insert(plugin.id().to_owned(), resolved);
        }
    }

    let toml = toml::to_string_pretty(&config)?;
    output::term().write_line(&toml)?;
    Ok(())
}
