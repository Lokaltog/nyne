//! Plugin configuration contract and top-level config loading.
//!
//! This module bridges the Tier-0 [`NyneConfig`] data layer and the Tier-3
//! [`Plugin`](crate::plugin::Plugin) trait: `PluginConfig` is the trait
//! plugin authors implement on their per-plugin config struct, and
//! `NyneConfig::load` composes the layered merge (core defaults → plugin
//! defaults → user TOML → project TOML) that needs access to the set of
//! linked plugins.

use std::any;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};

use crate::config::{NyneConfig, load_project_config, load_user_config};
use crate::deep_merge::deep_merge;
use crate::plugin::Plugin;

/// Trait for plugin configuration structs.
///
/// Provides a standard deserialization path from a layered TOML document.
/// Implement on your config struct (requires `Default + Serialize + Deserialize`),
/// then call `ctx.plugin_config::<Config>(self.id())` in your plugin's
/// `activate()` to materialize the resolved config.
///
/// Use `nyne::plugin_config!(ConfigType)` inside your `Plugin` impl to wire
/// `default_config()` and `resolved_config()` automatically.
pub trait PluginConfig: Default + serde::Serialize + for<'de> serde::Deserialize<'de> + Sized {
    /// Deserialize from an optional TOML section, falling back to defaults on
    /// missing section or deserialization failure. Logs a `warn!` with the
    /// type name and error when deserialization fails.
    fn from_section(section: Option<&toml::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        value.clone().try_into().unwrap_or_else(|err| {
            tracing::warn!(
                ?err,
                std_type = any::type_name::<Self>(),
                "invalid plugin config, using defaults"
            );
            Self::default()
        })
    }

    /// Serialize the default config as a TOML table for the merge chain.
    fn default_table() -> Option<toml::Table> { toml::Table::try_from(Self::default()).ok() }

    /// Serialize a resolved config instance as a TOML value for `nyne config` output.
    fn to_value(&self) -> Option<toml::Value> { toml::Value::try_from(self).ok() }
}

impl NyneConfig {
    /// Load configuration by merging layers in priority order.
    ///
    /// The merge strategy uses [`deep_merge`] so that each successive layer
    /// only needs to specify overrides — unset keys inherit from the layer
    /// below.
    ///
    /// ## Layer order (lowest → highest priority)
    ///
    /// 1. **Core defaults** — `NyneConfig::default()` serialized to TOML.
    /// 2. **Plugin defaults** — each plugin's `default_config()` merged into
    ///    `plugin.<id>`.
    /// 3. **User config** — XDG config file (`~/.config/nyne/config.toml`).
    /// 4. **Project config** — `.nyne.toml` (or similar) in the project root.
    ///
    /// After merging, the result is deserialized back into `NyneConfig` and
    /// validated with `garde`.
    ///
    /// # Errors
    ///
    /// Returns an error if any config file exists but cannot be read/parsed,
    /// if the merged result fails deserialization, or if validation fails.
    pub fn load(plugins: &[Box<dyn Plugin>], project_root: Option<&Path>) -> Result<Self> {
        use garde::Validate;

        // Layer 1: Core defaults.
        let mut merged = toml::Value::try_from(Self::default()).wrap_err("serializing default config")?;

        // Layer 2: Plugin defaults.
        for plugin in plugins {
            let Some(defaults) = plugin.default_config() else {
                continue;
            };

            deep_merge(
                merged
                    .get_mut("plugin")
                    .and_then(toml::Value::as_table_mut)
                    .ok_or_else(|| color_eyre::eyre::eyre!("default config missing plugin table"))?
                    .entry(plugin.id())
                    .or_insert(toml::Value::Table(toml::Table::new())),
                &toml::Value::Table(defaults),
            );
        }

        // Layer 3: User config (XDG).
        if let Some(user_config) = load_user_config()? {
            deep_merge(&mut merged, &user_config);
        }

        // Layer 4: Project config.
        if let Some(root) = project_root
            && let Some(project_config) = load_project_config(root)?
        {
            deep_merge(&mut merged, &project_config);
        }

        // Deserialize merged result.
        let config: Self = merged.try_into().wrap_err("deserializing merged config")?;
        config.validate().wrap_err("config validation failed")?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests;
