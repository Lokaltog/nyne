//! Generic immutable name→value registry, shared by [`ScriptRegistry`] and
//! [`ControlRegistry`].
//!
//! Both registries are populated once at mount time from pre-collected
//! entries contributed by plugins and then consulted by name for the
//! remainder of the daemon's lifetime. They share the same semantics
//! (O(1) lookup, duplicate-key warning, last-registration-wins) but have
//! distinct lookup signatures — scripts return [`Result`] with a
//! `not-found` error, control commands return [`Option`].
//!
//! This module factors out the HashMap + duplicate-warn bookkeeping so
//! both registries emit a consistent `duplicate registry entry` warning
//! labelled by kind.
//!
//! [`ScriptRegistry`]: super::ScriptRegistry
//! [`ControlRegistry`]: super::ControlRegistry

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;

use tracing::warn;

/// Immutable `HashMap`-backed registry with collision logging.
///
/// Constructed once via [`from_entries`](Self::from_entries); mutations
/// are not supported after construction. Every collision during
/// construction is logged at `warn!` level with `kind = <label>` so
/// downstream callers can filter by registry.
pub(crate) struct NamedRegistry<K, V> {
    entries: HashMap<K, V>,
}

impl<K, V> NamedRegistry<K, V>
where
    K: Eq + Hash + Display + Clone,
{
    /// Build a registry from pre-collected entries.
    ///
    /// `kind` is emitted as a structured field on the duplicate-warning
    /// log (e.g., `"script address"`, `"control command"`) so the same
    /// log target covers every registry.
    pub(crate) fn from_entries<I>(kind: &'static str, entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let mut map = HashMap::new();
        for (key, value) in entries {
            if map.insert(key.clone(), value).is_some() {
                warn!(kind, name = %key, "duplicate registry entry");
            }
        }
        Self { entries: map }
    }

    /// O(1) lookup by key.
    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key)
    }
}
