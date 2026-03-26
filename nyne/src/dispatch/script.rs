//! Script execution context, traits, and addressing.
//!
//! Scripts are in-process functions that accept binary stdin and produce binary
//! stdout, accessed via `nyne exec <address>`. They run inside the daemon with
//! full access to [`ActivationContext`] (git, syntax, LSP, etc.), making them
//! far cheaper than shelling out to external commands.
//!
//! Each script has a fully-qualified dotted address (e.g.,
//! `provider.coding.decompose`) built from namespace segments. Providers
//! register scripts during activation; the [`ScriptRegistry`](super::script_registry::ScriptRegistry)
//! indexes them for lookup by address.
//!
//! This is an **interface module** — the trait and address helpers may be
//! imported from any tier.

use std::borrow::Borrow;
use std::fmt;

use crate::prelude::*;

/// Namespace separator for script addresses (e.g., `provider.myplugin.my-script`).
pub const SCRIPT_NS_SEPARATOR: char = '.';

/// Namespace prefix for provider-registered scripts.
pub const SCRIPT_NS_PROVIDER: &str = "provider";

/// A validated, fully-qualified script address (e.g., `provider.myplugin.on-save`).
///
/// Script addresses are dot-separated with exactly three segments:
/// `{namespace}.{scope}.{name}`. Construction via [`script_address`] or
/// [`provider_script_address`] guarantees this format.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptAddress(String);

/// Validation and conversion for script addresses.
impl ScriptAddress {
    /// Create a new script address, validating that it contains exactly three
    /// dot-separated non-empty segments.
    pub fn new(address: impl Into<String>) -> Result<Self, ScriptAddressError> {
        let address = address.into();
        let segments: Vec<&str> = address.split(SCRIPT_NS_SEPARATOR).collect();
        if segments.len() != 3 || segments.iter().any(|s| s.is_empty()) {
            return Err(ScriptAddressError(address));
        }
        Ok(Self(address))
    }

    /// Return the address as a string slice.
    pub fn as_str(&self) -> &str { &self.0 }
}

impl fmt::Display for ScriptAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl Borrow<str> for ScriptAddress {
    fn borrow(&self) -> &str { &self.0 }
}

/// Error returned when a script address has an invalid format.
#[derive(Debug, thiserror::Error)]
#[error("invalid script address: {0:?} (expected three dot-separated non-empty segments)")]
pub struct ScriptAddressError(String);

/// Build a fully-qualified script address from namespace segments.
///
/// ```ignore
/// script_address("provider", "myplugin", "on-save") => "provider.myplugin.on-save"
/// ```
pub fn script_address(namespace: &str, scope: &str, name: &str) -> ScriptAddress {
    ScriptAddress(format!(
        "{namespace}{SCRIPT_NS_SEPARATOR}{scope}{SCRIPT_NS_SEPARATOR}{name}"
    ))
}

/// Build a provider-scoped script address: `provider.{id}.{name}`.
pub fn provider_script_address(provider_id: &str, name: &str) -> ScriptAddress {
    script_address(SCRIPT_NS_PROVIDER, provider_id, name)
}

/// Context available to scripts during execution.
///
/// Provides access to the full [`ActivationContext`] so scripts can query
/// project roots, configuration, and plugin-provided services (git, LSP, etc.)
/// without needing to shell out or duplicate setup logic.
pub struct ScriptContext<'a> {
    /// The shared activation context for this mount session.
    pub activation: &'a ActivationContext,
}

/// Construction and accessors for the script execution context.
impl<'a> ScriptContext<'a> {
    /// Create a new script context.
    pub(crate) const fn new(activation: &'a ActivationContext) -> Self { Self { activation } }

    /// Access the activation context (git, syntax, LSP, roots, etc.).
    pub const fn activation(&self) -> &ActivationContext { self.activation }
}

/// A named script that accepts stdin and produces stdout.
///
/// Scripts are pure functions: read stdin, access nyne state via
/// [`ScriptContext`], produce output bytes. They run in the daemon
/// process and have first-class access to all nyne infrastructure.
pub trait Script: Send + Sync {
    /// Execute the script with the given context and stdin bytes.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>>;
}

/// A registered script entry: fully-qualified address + implementation.
///
/// Returned by [`Provider::scripts`](crate::provider::Provider::scripts) during
/// activation. The address (first element) should be built via
/// [`provider_script_address`] for consistency.
pub type ScriptEntry = (ScriptAddress, Arc<dyn Script>);
