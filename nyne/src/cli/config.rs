//! `nyne config` — display the effective configuration.

use clap::Args;
use color_eyre::eyre::Result;

use super::output;
use crate::config::NyneConfig;
use crate::plugin::PLUGINS;

/// Show the effective configuration with all defaults resolved.
#[derive(Debug, Args)]
pub struct ConfigArgs;

/// Run the config subcommand.
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
