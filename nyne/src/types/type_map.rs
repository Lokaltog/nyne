//! `TypeId`-keyed heterogeneous map for typed property storage.
//!
//! Uses type erasure (`Box<dyn Any>`) so that plugins can attach arbitrary
//! data to nodes and contexts without the core library knowing concrete
//! types at compile time. The `TypeId` key guarantees that `get::<T>` only
//! succeeds when a `T` was previously inserted — no stringly-typed lookups,
//! no downcast guesswork.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// A `TypeId`-keyed heterogeneous map.
///
/// Stores at most one value per concrete type. Used by
/// [`VirtualNode`](crate::node::VirtualNode) for provider-specific properties
/// and by [`PipelineContext`](crate::dispatch::context::PipelineContext)
/// for middleware-to-middleware communication.
///
/// Values must be `Send + Sync + 'static`, so the map itself is safe to share
/// across threads behind an `Arc`. The map is **not** internally synchronized —
/// callers must provide their own locking if concurrent mutation is needed.
#[derive(Default)]
pub struct TypeMap {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

/// Typed insertion and retrieval for the heterogeneous map.
impl TypeMap {
    /// Create an empty map.
    pub fn new() -> Self { Self::default() }

    /// Insert a typed value. Replaces any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieve a reference to a typed value.
    pub fn get<T: 'static>(&self) -> Option<&T> { self.inner.get(&TypeId::of::<T>())?.downcast_ref() }

    /// Merge entries from `other` into `self`. Existing keys in `self` are
    /// preserved (first-writer-wins); only missing keys are filled from `other`.
    pub fn merge_from(&mut self, other: Self) {
        for (key, value) in other.inner {
            self.inner.entry(key).or_insert(value);
        }
    }
}
